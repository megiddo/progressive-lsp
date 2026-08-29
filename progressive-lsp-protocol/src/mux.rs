//! Optional `--mux` Adapter: channel id + length + payload on one stdio pipe.
//!
//! Channel `lsp` (0) carries opaque JSON-RPC bytes (not protobuf).
//! Channel `control` (1) carries length-prefixed `progressive.v1` protobuf
//! (same inner framing as a dedicated control socket).

use std::io::{Read, Write};

/// LSP JSON-RPC channel. Payload is the JSON-RPC message body (no Content-Length).
pub const CHANNEL_LSP: u8 = 0;
/// Control protobuf channel. Payload is `u32be | proto` ([`progressive_lsp_control`] frames).
pub const CHANNEL_CONTROL: u8 = 1;

/// Same 16 MiB cap as the control codec. Exceeding this fails (no silent truncate).
pub const MAX_MUX_PAYLOAD: u32 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum MuxError {
    #[error("mux payload exceeds {MAX_MUX_PAYLOAD} bytes ({0})")]
    PayloadTooLarge(u32),
    #[error("unknown mux channel {0}")]
    UnknownChannel(u8),
    #[error("incomplete mux frame")]
    Incomplete,
    #[error("io: {0}")]
    Io(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MuxFrame {
    pub channel: u8,
    pub payload: Vec<u8>,
}

impl MuxFrame {
    pub fn lsp(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            channel: CHANNEL_LSP,
            payload: payload.into(),
        }
    }

    pub fn control(payload: impl Into<Vec<u8>>) -> Self {
        Self {
            channel: CHANNEL_CONTROL,
            payload: payload.into(),
        }
    }

    pub fn is_lsp(&self) -> bool {
        self.channel == CHANNEL_LSP
    }

    pub fn is_control(&self) -> bool {
        self.channel == CHANNEL_CONTROL
    }
}

pub fn encode_mux_frame(channel: u8, payload: &[u8]) -> Result<Vec<u8>, MuxError> {
    if channel != CHANNEL_LSP && channel != CHANNEL_CONTROL {
        return Err(MuxError::UnknownChannel(channel));
    }
    let len = u32::try_from(payload.len()).map_err(|_| MuxError::PayloadTooLarge(u32::MAX))?;
    if len > MAX_MUX_PAYLOAD {
        return Err(MuxError::PayloadTooLarge(len));
    }
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(channel);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

pub fn decode_mux_frame(buf: &[u8]) -> Result<Option<(MuxFrame, usize)>, MuxError> {
    if buf.is_empty() {
        return Ok(None);
    }
    if buf.len() < 5 {
        return Err(MuxError::Incomplete);
    }
    let channel = buf[0];
    if channel != CHANNEL_LSP && channel != CHANNEL_CONTROL {
        return Err(MuxError::UnknownChannel(channel));
    }
    let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]);
    if len > MAX_MUX_PAYLOAD {
        return Err(MuxError::PayloadTooLarge(len));
    }
    let total = 5 + len as usize;
    if buf.len() < total {
        return Err(MuxError::Incomplete);
    }
    Ok(Some((
        MuxFrame {
            channel,
            payload: buf[5..total].to_vec(),
        },
        total,
    )))
}

pub fn write_mux_frame<W: Write>(
    writer: &mut W,
    channel: u8,
    payload: &[u8],
) -> Result<(), MuxError> {
    let framed = encode_mux_frame(channel, payload)?;
    writer
        .write_all(&framed)
        .map_err(|e| MuxError::Io(e.to_string()))?;
    writer.flush().map_err(|e| MuxError::Io(e.to_string()))?;
    Ok(())
}

pub fn read_mux_frame<R: Read>(reader: &mut R) -> Result<Option<MuxFrame>, MuxError> {
    let mut header = [0u8; 5];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(MuxError::Io(e.to_string())),
    }
    let channel = header[0];
    if channel != CHANNEL_LSP && channel != CHANNEL_CONTROL {
        return Err(MuxError::UnknownChannel(channel));
    }
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]);
    if len > MAX_MUX_PAYLOAD {
        return Err(MuxError::PayloadTooLarge(len));
    }
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        reader
            .read_exact(&mut payload)
            .map_err(|e| MuxError::Io(e.to_string()))?;
    }
    Ok(Some(MuxFrame { channel, payload }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_is_channel_then_u32be_then_payload() {
        let framed = encode_mux_frame(CHANNEL_LSP, b"{\"a\":1}").unwrap();
        assert_eq!(framed[0], CHANNEL_LSP);
        assert_eq!(&framed[1..5], &(7u32).to_be_bytes());
        assert_eq!(&framed[5..], b"{\"a\":1}");
        let (frame, n) = decode_mux_frame(&framed).unwrap().unwrap();
        assert_eq!(n, framed.len());
        assert!(frame.is_lsp());
        assert!(!frame.is_control());
        assert_eq!(frame.payload, b"{\"a\":1}");
    }

    #[test]
    fn control_channel_round_trip() {
        let inner = [0, 0, 0, 1, 0xAB];
        let framed = encode_mux_frame(CHANNEL_CONTROL, &inner).unwrap();
        let (frame, _) = decode_mux_frame(&framed).unwrap().unwrap();
        assert!(frame.is_control());
        assert!(!frame.is_lsp());
        assert_eq!(frame.payload, inner);
        assert_eq!(MuxFrame::control(inner.to_vec()).channel, CHANNEL_CONTROL);
        assert_eq!(MuxFrame::lsp(b"x".to_vec()).channel, CHANNEL_LSP);
    }

    #[test]
    fn empty_buffer_is_eof() {
        assert!(decode_mux_frame(&[]).unwrap().is_none());
        assert!(read_mux_frame(&mut Cursor::new(Vec::<u8>::new()))
            .unwrap()
            .is_none());
    }

    #[test]
    fn incomplete_and_unknown_and_too_large() {
        assert!(matches!(
            decode_mux_frame(&[CHANNEL_LSP, 0, 0]),
            Err(MuxError::Incomplete)
        ));
        assert!(matches!(
            decode_mux_frame(&[9, 0, 0, 0, 0]),
            Err(MuxError::UnknownChannel(9))
        ));
        assert!(encode_mux_frame(9, b"x").is_err());
        let too = MAX_MUX_PAYLOAD + 1;
        let mut hdr = vec![CHANNEL_LSP];
        hdr.extend_from_slice(&too.to_be_bytes());
        assert!(matches!(
            decode_mux_frame(&hdr),
            Err(MuxError::PayloadTooLarge(n)) if n == too
        ));
        let huge = vec![0u8; (MAX_MUX_PAYLOAD as usize) + 1];
        assert!(encode_mux_frame(CHANNEL_LSP, &huge).is_err());
        assert!(decode_mux_frame(&[CHANNEL_LSP, 0, 0, 0, 2, 1]).is_err());
        assert_eq!(MAX_MUX_PAYLOAD, 16 * 1024 * 1024);
        assert!(MuxError::Incomplete.to_string().contains("incomplete"));
        assert!(MuxError::Io("e".into()).to_string().contains("e"));
    }

    #[test]
    fn read_write_round_trip() {
        let mut out = Vec::new();
        write_mux_frame(&mut out, CHANNEL_LSP, b"hi").unwrap();
        write_mux_frame(&mut out, CHANNEL_CONTROL, b"proto").unwrap();
        let mut cur = Cursor::new(out);
        let a = read_mux_frame(&mut cur).unwrap().unwrap();
        assert_eq!(a.payload, b"hi");
        let b = read_mux_frame(&mut cur).unwrap().unwrap();
        assert_eq!(b.payload, b"proto");
        assert!(read_mux_frame(&mut cur).unwrap().is_none());
    }

    #[test]
    fn empty_payload_is_five_bytes() {
        let framed = encode_mux_frame(CHANNEL_LSP, b"").unwrap();
        assert_eq!(framed, vec![CHANNEL_LSP, 0, 0, 0, 0]);
        let (frame, n) = decode_mux_frame(&framed).unwrap().unwrap();
        assert_eq!(n, 5);
        assert!(frame.payload.is_empty());
    }
}
