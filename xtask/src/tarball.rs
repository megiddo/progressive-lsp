//! Minimal ustar writer. No gzip. Host Darwin writes this layout; payloads may be stubs.

use std::io::Write;
use std::path::{Path, PathBuf};

const BLOCK: usize = 512;

pub fn write_ustar(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (name, bytes) in files {
        if name.len() >= 100 {
            return Err(format!("ustar name too long: {name}"));
        }
        if name.contains("..") || name.starts_with('/') {
            return Err(format!("ustar name must be relative: {name}"));
        }
        out.extend_from_slice(&ustar_header(name, bytes.len() as u64)?);
        out.extend_from_slice(bytes);
        let pad = (BLOCK - (bytes.len() % BLOCK)) % BLOCK;
        out.extend(std::iter::repeat(0u8).take(pad));
    }
    out.extend_from_slice(&[0u8; BLOCK]);
    out.extend_from_slice(&[0u8; BLOCK]);
    Ok(out)
}

fn ustar_header(name: &str, size: u64) -> Result<[u8; BLOCK], String> {
    let mut h = [0u8; BLOCK];
    write_str(&mut h[0..100], name);
    write_octal(&mut h[100..108], 0o644);
    write_octal(&mut h[108..116], 0);
    write_octal(&mut h[116..124], 0);
    write_octal(&mut h[124..136], size);
    write_octal(&mut h[136..148], 0);
    h[156] = b'0';
    write_str(&mut h[257..263], "ustar");
    h[263] = 0;
    write_str(&mut h[264..266], "00");
    for b in &mut h[148..156] {
        *b = b' ';
    }
    let sum: u32 = h.iter().map(|b| *b as u32).sum();
    write_octal(&mut h[148..155], u64::from(sum));
    h[155] = b' ';
    Ok(h)
}

fn write_str(slot: &mut [u8], s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(slot.len());
    slot[..n].copy_from_slice(&bytes[..n]);
}

fn write_octal(slot: &mut [u8], value: u64) {
    let width = slot.len().saturating_sub(1);
    let formatted = format!("{value:0width$o}");
    let bytes = formatted.as_bytes();
    let n = bytes.len().min(width);
    slot[..n].copy_from_slice(&bytes[..n]);
    if slot.len() > n {
        slot[n] = 0;
    }
}

pub fn collect_dir(root: &Path, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    walk(root, prefix, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for ent in rd {
        let ent = ent.map_err(|e| format!("dirent: {e}"))?;
        let path = ent.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name == "dist" {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            walk(&path, &rel, out)?;
        } else if path.is_file() {
            let bytes =
                std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            out.push((rel, bytes));
        }
    }
    Ok(())
}

pub fn write_tar_file(path: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let bytes = write_ustar(files)?;
    let mut f = std::fs::File::create(path)
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    f.write_all(&bytes)
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

pub fn sha256_sidecar(tar_path: &Path, hex: &str) -> Result<PathBuf, String> {
    let name = tar_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("tarball has no file name")?;
    let side = tar_path.with_extension("tar.sha256");
    let side = if tar_path.extension().and_then(|s| s.to_str()) == Some("tar") {
        PathBuf::from(format!("{}.sha256", tar_path.display()))
    } else {
        side
    };
    std::fs::write(&side, format!("{hex}  {name}\n"))
        .map_err(|e| format!("write {}: {e}", side.display()))?;
    Ok(side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ustar_round_trip_layout() {
        let tar = write_ustar(&[
            ("DIST_README.txt".into(), b"stub note\n".to_vec()),
            ("engines/python/ty".into(), b"pack-stub\n".to_vec()),
        ])
        .unwrap();
        assert!(tar.len() >= BLOCK * 4);
        assert_eq!(&tar[0..15], b"DIST_README.txt");
        assert!(tar.windows(b"engines/python/ty".len()).any(|w| w == b"engines/python/ty"));
        assert!(write_ustar(&[("../x".into(), b"z".to_vec())]).is_err());
        assert!(write_ustar(&[("/abs".into(), b"z".to_vec())]).is_err());
        assert!(write_ustar(&[("a".repeat(100), b"z".to_vec())]).is_err());
    }

    #[test]
    fn collect_skips_dist_subdir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("engines/python")).unwrap();
        std::fs::write(dir.path().join("engines/python/ty"), b"stub").unwrap();
        std::fs::create_dir_all(dir.path().join("dist/x")).unwrap();
        std::fs::write(dir.path().join("dist/x/nope"), b"no").unwrap();
        let files = collect_dir(dir.path(), "").unwrap();
        assert!(files.iter().any(|(n, _)| n == "engines/python/ty"));
        assert!(!files.iter().any(|(n, _)| n.contains("dist/")));
        let tar_path = dir.path().join("out.tar");
        write_tar_file(&tar_path, &files).unwrap();
        assert!(tar_path.is_file());
        let side = sha256_sidecar(&tar_path, "abcd").unwrap();
        assert!(side.to_string_lossy().ends_with(".tar.sha256"));
        assert!(std::fs::read_to_string(&side).unwrap().contains("out.tar"));
    }
}
