//! Length-prefixed frames: `u32be payload_length | protobuf bytes`.

use thiserror::Error;

/// Documented max payload (16 MiB). Exceeding this fails the request.
pub const MAX_PAYLOAD_BYTES: u32 = 16 * 1024 * 1024;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CodecError {
    #[error("payload exceeds {MAX_PAYLOAD_BYTES} bytes ({0})")]
    PayloadTooLarge(u32),
    #[error("incomplete frame")]
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeOutcome {
    Incomplete { needed: u32 },
    Complete { payload: Vec<u8>, consumed: usize },
}

pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, CodecError> {
    let len = u32::try_from(payload.len()).map_err(|_| {
        CodecError::PayloadTooLarge(u32::MAX)
    })?;
    if len > MAX_PAYLOAD_BYTES {
        return Err(CodecError::PayloadTooLarge(len));
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

pub fn decode_frame(buf: &[u8]) -> Result<DecodeOutcome, CodecError> {
    if buf.len() < 4 {
        return Ok(DecodeOutcome::Incomplete {
            needed: 4 - buf.len() as u32,
        });
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if len > MAX_PAYLOAD_BYTES {
        return Err(CodecError::PayloadTooLarge(len));
    }
    let total = 4 + len as usize;
    if buf.len() < total {
        return Ok(DecodeOutcome::Incomplete {
            needed: (total - buf.len()) as u32,
        });
    }
    Ok(DecodeOutcome::Complete {
        payload: buf[4..total].to_vec(),
        consumed: total,
    })
}

/// Strict helper: require a complete frame occupying the whole buffer.
pub fn decode_exact(buf: &[u8]) -> Result<Vec<u8>, CodecError> {
    match decode_frame(buf)? {
        DecodeOutcome::Complete { payload, consumed } if consumed == buf.len() => Ok(payload),
        DecodeOutcome::Complete { .. } => Err(CodecError::Incomplete),
        DecodeOutcome::Incomplete { .. } => Err(CodecError::Incomplete),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefix_is_big_endian() {
        let frame = encode_frame(&[0xAB]).unwrap();
        assert_eq!(&frame[..4], &[0, 0, 0, 1]);
        assert_eq!(frame[4], 0xAB);
        match decode_frame(&frame).unwrap() {
            DecodeOutcome::Complete { payload, consumed } => {
                assert_eq!(payload, vec![0xAB]);
                assert_eq!(consumed, 5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_payload_is_four_zero_bytes() {
        let frame = encode_frame(&[]).unwrap();
        assert_eq!(frame, [0, 0, 0, 0]);
        assert_eq!(decode_exact(&frame).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn incomplete_header_and_body() {
        assert_eq!(
            decode_frame(&[0, 0]).unwrap(),
            DecodeOutcome::Incomplete { needed: 2 }
        );
        let mut prefix = (2u32).to_be_bytes().to_vec();
        prefix.push(1);
        assert_eq!(
            decode_frame(&prefix).unwrap(),
            DecodeOutcome::Incomplete { needed: 1 }
        );
        assert!(matches!(decode_exact(&[0, 0]), Err(CodecError::Incomplete)));
    }

    #[test]
    fn decode_exact_rejects_trailing_bytes() {
        let mut frame = encode_frame(&[1, 2]).unwrap();
        frame.push(9);
        assert!(matches!(decode_exact(&frame), Err(CodecError::Incomplete)));
    }

    #[test]
    fn max_payload_rejected_on_encode_and_decode() {
        let too_big_len = MAX_PAYLOAD_BYTES + 1;
        let header = too_big_len.to_be_bytes();
        assert!(matches!(
            decode_frame(&header),
            Err(CodecError::PayloadTooLarge(n)) if n == too_big_len
        ));
        let huge = vec![0u8; (MAX_PAYLOAD_BYTES as usize) + 1];
        assert!(matches!(
            encode_frame(&huge),
            Err(CodecError::PayloadTooLarge(n)) if n == too_big_len
        ));
    }

    #[test]
    fn max_payload_accepted() {
        let payload = vec![7u8; MAX_PAYLOAD_BYTES as usize];
        let frame = encode_frame(&payload).unwrap();
        assert_eq!(frame.len(), 4 + MAX_PAYLOAD_BYTES as usize);
        assert_eq!(decode_exact(&frame).unwrap(), payload);
    }

    #[test]
    fn sixteen_mib_constant() {
        assert_eq!(MAX_PAYLOAD_BYTES, 16 * 1024 * 1024);
        assert_eq!(
            CodecError::PayloadTooLarge(1).to_string(),
            format!("payload exceeds {MAX_PAYLOAD_BYTES} bytes (1)")
        );
        assert_eq!(CodecError::Incomplete.to_string(), "incomplete frame");
    }
}
