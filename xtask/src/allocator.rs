//! `xtask bench-alloc` reads `allocator-matrix.toml`. Empty cells → mimalloc.

use std::fs;

use crate::workspace_root;

pub const MATRIX_REL: &str = "xtask/allocator-matrix.toml";

pub fn matrix_path() -> std::path::PathBuf {
    workspace_root().join(MATRIX_REL)
}

pub fn run(_args: &[String]) -> Result<(), String> {
    let path = matrix_path();
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| format!("parse matrix: {e}"))?;
    let cells = table
        .get("cell")
        .and_then(|v| v.as_array())
        .ok_or("allocator-matrix.toml must have [[cell]] entries")?;
    if cells.is_empty() {
        return Err("allocator-matrix.toml has no cells".into());
    }
    for cell in cells {
        let backend = cell.get("backend").and_then(|v| v.as_str()).unwrap_or("");
        let arch = cell.get("arch").and_then(|v| v.as_str()).unwrap_or("");
        let libc = cell.get("libc").and_then(|v| v.as_str()).unwrap_or("");
        let allocator = cell
            .get("allocator")
            .and_then(|v| v.as_str())
            .unwrap_or("mimalloc");
        let source = cell.get("source").and_then(|v| v.as_str()).unwrap_or("");
        if backend.is_empty() || arch.is_empty() || libc.is_empty() {
            return Err("cell missing backend/arch/libc".into());
        }
        if source != "ci" && allocator != "mimalloc" {
            return Err(format!(
                "empty/placeholder cell ({backend},{arch},{libc}) must use mimalloc, got {allocator}"
            ));
        }
        println!("{backend} {arch} {libc} -> {allocator} ({source})");
    }
    println!(
        "pick rule: lowest p99; tie within 5% p99 and 5% RSS → mimalloc, jemalloc, libc. \
         dist only reads this file."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_placeholders_are_mimalloc() {
        run(&[]).unwrap();
        assert!(matrix_path().is_file());
        assert_eq!(MATRIX_REL, "xtask/allocator-matrix.toml");
    }
}
