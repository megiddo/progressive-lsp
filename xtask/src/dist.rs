//! `xtask dist --pack python,rust`: engines dest + manifest + hash.
//! Real ty/rust-analyzer musl ELFs are Linux CI / Docker. Darwin writes stubs only.

use std::path::{Path, PathBuf};

use progressive_lsp_engine::{
    hex_of, pack_dir, stub_pack_bytes, PYTHON_PACK, RA_BINARY, RUST_PACK, TY_BINARY,
};
use progressive_lsp_install::{Manifest, ManifestArtifact};

pub fn run(args: &[String]) -> Result<(), String> {
    let mut packs: Vec<String> = Vec::new();
    let mut dest: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pack" | "--packs" => {
                i += 1;
                let raw = args.get(i).ok_or("--pack requires python,rust")?;
                packs = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "--dest" | "--prefix" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            other => return Err(format!("unknown dist flag: {other}")),
        }
        i += 1;
    }
    if packs.is_empty() {
        return Err("dist requires --pack python,rust".into());
    }
    let dest = dest.ok_or("dist requires --dest DIR")?;
    write_packs(&dest, &packs)
}

pub fn write_packs(dest: &Path, packs: &[String]) -> Result<(), String> {
    let prefix = progressive_lsp_core::PrefixLayout::from_path(dest);
    prefix
        .ensure_dirs()
        .map_err(|e| format!("ensure prefix: {e}"))?;
    let mut note = String::from(
        "Darwin / local `xtask dist --pack` writes pack stubs + manifest hashes only.\n\
         Real ty and rust-analyzer musl ELFs (no interpreter, no DT_NEEDED) are built in Linux CI / Docker.\n\
         Do not treat these stubs as check-static greens.\n",
    );
    for pack in packs {
        match pack.as_str() {
            PYTHON_PACK => {
                write_one(&prefix, PYTHON_PACK, TY_BINARY)?;
                note.push_str("pack=python binary=ty\n");
            }
            RUST_PACK => {
                write_one(&prefix, RUST_PACK, RA_BINARY)?;
                note.push_str("pack=rust binary=rust-analyzer\n");
            }
            other => return Err(format!("M3 dist supports python,rust only; got {other}")),
        }
    }
    std::fs::write(prefix.engines_dir().join("DARWIN_CI_GAP.txt"), note)
        .map_err(|e| format!("write gap note: {e}"))?;
    Ok(())
}

fn write_one(
    prefix: &progressive_lsp_core::PrefixLayout,
    pack: &str,
    binary: &str,
) -> Result<(), String> {
    let dir = pack_dir(prefix, pack);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    let bytes = stub_pack_bytes(pack, binary);
    let sha = hex_of(&bytes);
    std::fs::write(dir.join(binary), &bytes).map_err(|e| format!("write {binary}: {e}"))?;
    let manifest = Manifest {
        version: "1".into(),
        artifacts: vec![ManifestArtifact {
            name: binary.into(),
            rel_path: binary.into(),
            sha256: sha,
            executable: true,
        }],
    };
    std::fs::write(dir.join("manifest.json"), manifest.to_json().map_err(|e| e.to_string())?)
        .map_err(|e| format!("write manifest: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_engine::{discover_pack, is_pack_stub};

    #[test]
    fn dist_pack_writes_manifest_and_hash() {
        let dir = tempfile::tempdir().unwrap();
        run(&[
            "--pack".into(),
            "python,rust".into(),
            "--dest".into(),
            dir.path().display().to_string(),
        ])
        .unwrap();
        let prefix = progressive_lsp_core::PrefixLayout::from_path(dir.path());
        let py = discover_pack(&prefix, PYTHON_PACK).unwrap();
        let rs = discover_pack(&prefix, RUST_PACK).unwrap();
        assert!(is_pack_stub(&std::fs::read(&py.path).unwrap()));
        assert!(is_pack_stub(&std::fs::read(&rs.path).unwrap()));
        assert!(prefix.engines_dir().join("DARWIN_CI_GAP.txt").is_file());
        assert!(run(&[]).is_err());
        assert!(run(&["--pack".into()]).is_err());
        assert!(run(&["--nope".into()]).is_err());
        assert!(run(&[
            "--pack".into(),
            "clangd".into(),
            "--dest".into(),
            dir.path().display().to_string()
        ])
        .is_err());
        assert!(run(&["--dest".into()]).is_err());
        assert!(run(&["--pack".into(), "python".into()]).is_err());
    }
}
