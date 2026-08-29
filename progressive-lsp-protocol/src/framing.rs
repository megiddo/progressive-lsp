//! LSP Content-Length framing.

use std::io::{BufRead, Write};

#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("missing Content-Length header")]
    MissingContentLength,
    #[error("invalid Content-Length: {0}")]
    InvalidContentLength(String),
    #[error("header line too long")]
    HeaderTooLong,
}

const MAX_HEADER_LINE: usize = 4096;

pub fn encode_message(body: impl AsRef<[u8]>) -> Vec<u8> {
    let body = body.as_ref();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body);
    out
}

pub fn write_message<W: Write>(writer: &mut W, body: impl AsRef<[u8]>) -> Result<(), FramingError> {
    writer.write_all(&encode_message(body))?;
    writer.flush()?;
    Ok(())
}

pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, FramingError> {
    let mut content_length: Option<usize> = None;
    let mut saw_any = false;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return if saw_any {
                Err(FramingError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "eof mid-headers",
                )))
            } else {
                Ok(None)
            };
        }
        if line.len() > MAX_HEADER_LINE {
            return Err(FramingError::HeaderTooLong);
        }
        saw_any = true;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                let parsed = value.trim().parse::<usize>().map_err(|_| {
                    FramingError::InvalidContentLength(value.trim().to_string())
                })?;
                content_length = Some(parsed);
            }
        }
    }
    let len = content_length.ok_or(FramingError::MissingContentLength)?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(buf))
}

pub fn decode_all(bytes: &[u8]) -> Result<Vec<Vec<u8>>, FramingError> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut out = Vec::new();
    while let Some(msg) = read_message(&mut cursor)? {
        out.push(msg);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn encode_uses_crlf_and_length() {
        let framed = encode_message(b"hi");
        assert_eq!(&framed, b"Content-Length: 2\r\n\r\nhi");
    }

    #[test]
    fn read_round_trip_and_optional_content_type() {
        let mut raw = b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".to_vec();
        raw.extend_from_slice(&encode_message(b"{\"a\":1}"));
        let mut cur = Cursor::new(raw);
        let body = read_message(&mut cur).unwrap().unwrap();
        assert_eq!(body, b"{\"a\":1}");
        assert!(read_message(&mut cur).unwrap().is_none());
    }

    #[test]
    fn missing_content_length_is_error() {
        let err = read_message(&mut Cursor::new(b"X: 1\r\n\r\n")).unwrap_err();
        assert!(matches!(err, FramingError::MissingContentLength));
    }

    #[test]
    fn invalid_content_length_is_error() {
        let err = read_message(&mut Cursor::new(b"Content-Length: nope\r\n\r\n")).unwrap_err();
        assert!(matches!(err, FramingError::InvalidContentLength(_)));
    }

    #[test]
    fn header_too_long_is_error() {
        let line = format!("X: {}\r\n", "a".repeat(MAX_HEADER_LINE));
        let err = read_message(&mut Cursor::new(line)).unwrap_err();
        assert!(matches!(err, FramingError::HeaderTooLong));
    }

    #[test]
    fn eof_mid_headers_is_error() {
        let err = read_message(&mut Cursor::new(b"Content-Length: 1")).unwrap_err();
        assert!(matches!(err, FramingError::Io(_)));
    }

    #[test]
    fn write_message_flushes() {
        let mut out = Vec::new();
        write_message(&mut out, b"z").unwrap();
        assert_eq!(out, encode_message(b"z"));
    }

    #[test]
    fn decode_all_two_messages() {
        let mut bytes = encode_message(b"a");
        bytes.extend_from_slice(&encode_message(b"bb"));
        let all = decode_all(&bytes).unwrap();
        assert_eq!(all, vec![b"a".to_vec(), b"bb".to_vec()]);
    }

    #[test]
    fn content_length_is_case_insensitive() {
        let mut cur = Cursor::new(*b"content-length: 1\r\n\r\nZ");
        assert_eq!(read_message(&mut cur).unwrap().unwrap(), b"Z");
    }
}
