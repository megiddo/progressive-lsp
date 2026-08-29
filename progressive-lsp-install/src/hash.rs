//! SHA-256 helpers. Hex is lowercase.

use std::path::Path;

use progressive_lsp_core::InstallError;
use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn sha256_file(path: &Path) -> Result<[u8; 32], InstallError> {
    let bytes = std::fs::read(path)
        .map_err(|e| InstallError::Io(format!("read {}: {e}", path.display())))?;
    Ok(sha256(&bytes))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>, InstallError> {
    if s.len() % 2 != 0 {
        return Err(InstallError::Manifest(format!("odd hex length: {s}")));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, InstallError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(InstallError::Manifest(format!(
            "invalid hex digit: {}",
            b as char
        ))),
    }
}

pub fn verify_hash(actual: &[u8; 32], expected_hex: &str) -> Result<(), InstallError> {
    let expected = hex_decode(expected_hex)?;
    if expected.len() != 32 {
        return Err(InstallError::Manifest(format!(
            "sha256 must be 32 bytes, got {}",
            expected.len()
        )));
    }
    if actual.as_slice() != expected.as_slice() {
        return Err(InstallError::Hash {
            expected: expected_hex.to_ascii_lowercase(),
            actual: hex_encode(actual),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_empty_is_known_vector() {
        let digest = sha256(b"");
        assert_eq!(
            hex_encode(&digest),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hex_round_trip_and_case() {
        let bytes = hex_decode("00Ff0a").unwrap();
        assert_eq!(bytes, vec![0x00, 0xff, 0x0a]);
        assert_eq!(hex_encode(&bytes), "00ff0a");
        assert!(hex_decode("0").is_err());
        assert!(hex_decode("zz").is_err());
        assert!(hex_decode("0g").is_err());
    }

    #[test]
    fn verify_hash_ok_and_mismatch() {
        let d = sha256(b"abc");
        verify_hash(&d, &hex_encode(&d)).unwrap();
        let err = verify_hash(&d, &hex_encode(&sha256(b"xyz"))).unwrap_err();
        assert!(matches!(err, InstallError::Hash { .. }));
        assert!(verify_hash(&d, "aa").is_err());
    }

    #[test]
    fn sha256_file_reads_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256(b"abc"));
        assert!(sha256_file(&dir.path().join("missing")).is_err());
    }
}
