//! `file:` URI Adapter. POC IDE percent-encodes paths; ingest uses OS paths.
//! Decode incoming URIs and encode outgoing locations so `FileId` matches.

use std::path::{Path, PathBuf};

/// Absolute (or already-`file:`) path → `file:` URI. Spaces, `@`, and other
/// reserved bytes become `%XX`. `/` is left intact so `file:///a/b` stays valid.
pub fn path_to_file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if raw.starts_with("file:") {
        return raw.into_owned();
    }
    let mut out = String::from("file://");
    for b in raw.as_bytes() {
        match *b {
            b'/' | b'-' | b'_' | b'.' | b'~' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `file:` URI (percent-encoded or not) → OS path. Strips `file://`,
/// optional `localhost`, and query/fragment. Non-`file:` strings are decoded
/// in place so `/abs` and encoded paths both work.
pub fn path_from_file_uri(uri: &str) -> PathBuf {
    let Some(rest) = uri.strip_prefix("file://") else {
        return PathBuf::from(percent_decode(uri));
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    if decoded.starts_with('/') || decoded.is_empty() {
        PathBuf::from(decoded)
    } else {
        PathBuf::from(format!("/{decoded}"))
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(h) = hex_byte(bytes[i + 1], bytes[i + 2]) {
                out.push(h);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_byte(a: u8, b: u8) -> Option<u8> {
    Some((hex_val(a)? << 4) | hex_val(b)?)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileId;
    use std::path::Path;

    #[test]
    fn file_uri_roundtrip_encodes_space_and_at() {
        let path = Path::new("/Users/me/GoogleDrive-en.gannim@gmail.com/My Drive/App.java");
        let uri = path_to_file_uri(path);
        assert_eq!(
            uri,
            "file:///Users/me/GoogleDrive-en.gannim%40gmail.com/My%20Drive/App.java"
        );
        assert_eq!(path_from_file_uri(&uri), path);
        assert_eq!(
            FileId::from_uri(&uri).as_str(),
            path.to_string_lossy().as_ref()
        );
    }

    #[test]
    fn file_uri_plain_and_already_file_scheme() {
        assert_eq!(path_to_file_uri(Path::new("/tmp/a")), "file:///tmp/a");
        assert_eq!(path_to_file_uri(Path::new("file://x")), "file://x");
        assert_eq!(path_from_file_uri("file:///tmp/a"), PathBuf::from("/tmp/a"));
        assert_eq!(path_from_file_uri("/abs"), PathBuf::from("/abs"));
        assert_eq!(
            path_from_file_uri("file://localhost/ws/a.rs"),
            PathBuf::from("/ws/a.rs")
        );
        assert_eq!(
            path_from_file_uri("file:///ws/a.rs?x=1#frag"),
            PathBuf::from("/ws/a.rs")
        );
        assert_eq!(
            path_from_file_uri("file://host/x"),
            PathBuf::from("/host/x")
        );
        assert_eq!(path_from_file_uri(""), PathBuf::from(""));
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%2"), "%2");
        assert_eq!(percent_decode("A%2fb"), "A/b");
        assert_eq!(hex_val(b'0'), Some(0));
        assert_eq!(hex_val(b'g'), None);
        assert_eq!(hex_byte(b'2', b'F'), Some(0x2f));
        assert_eq!(hex_byte(b'z', b'z'), None);
    }
}
