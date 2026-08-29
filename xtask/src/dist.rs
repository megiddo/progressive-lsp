//! `xtask dist --pack slim|full|python,rust,...`: engines dest + manifest + hash.
//! Real musl ELFs are Linux CI / Docker. Darwin writes stubs only.
//! Slim (default) excludes clangd/tsgo/gopls/zls for Java-only workspaces.

use std::path::{Path, PathBuf};

use progressive_lsp_engine::{
    binary_name_for_pack, full_pack_names, hex_of, is_heavy_pack, pack_dir, slim_pack_names,
    stub_pack_bytes, CLANGD_PACK, GOPLS_PACK, TSGO_PACK, ZLS_PACK,
};
use progressive_lsp_install::{Manifest, ManifestArtifact};

pub fn run(args: &[String]) -> Result<(), String> {
    let mut packs: Vec<String> = Vec::new();
    let mut dest: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--pack" | "--packs" | "--flavor" => {
                i += 1;
                let raw = args.get(i).ok_or("--pack requires slim, full, or a CSV")?;
                packs = expand_pack_list(raw);
            }
            "--dest" | "--prefix" => {
                i += 1;
                dest = Some(PathBuf::from(args.get(i).ok_or("--dest requires a path")?));
            }
            "--slim" => packs = slim_pack_names().iter().map(|s| (*s).to_string()).collect(),
            "--full" => packs = full_pack_names().iter().map(|s| (*s).to_string()).collect(),
            other => return Err(format!("unknown dist flag: {other}")),
        }
        i += 1;
    }
    if packs.is_empty() {
        packs = slim_pack_names().iter().map(|s| (*s).to_string()).collect();
    }
    let dest = dest.ok_or("dist requires --dest DIR")?;
    write_packs(&dest, &packs)
}

fn expand_pack_list(raw: &str) -> Vec<String> {
    match raw.trim() {
        "slim" => slim_pack_names().iter().map(|s| (*s).to_string()).collect(),
        "full" => full_pack_names().iter().map(|s| (*s).to_string()).collect(),
        other => other
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    }
}

pub fn write_packs(dest: &Path, packs: &[String]) -> Result<(), String> {
    let prefix = progressive_lsp_core::PrefixLayout::from_path(dest);
    prefix
        .ensure_dirs()
        .map_err(|e| format!("ensure prefix: {e}"))?;
    let mut note = String::from(
        "Darwin / local `xtask dist` writes pack stubs + manifest hashes only.\n\
         Real engine musl ELFs (no interpreter, no DT_NEEDED) are built in Linux CI / Docker.\n\
         Do not treat these stubs as check-static greens.\n\
         Slim default excludes clangd, tsgo, gopls, zls.\n",
    );
    for pack in packs {
        let binary = binary_name_for_pack(pack)
            .ok_or_else(|| format!("unknown pack {pack}; known: slim, full, or named packs"))?;
        write_one(&prefix, pack, binary)?;
        note.push_str(&format!("pack={pack} binary={binary}\n"));
        if is_heavy_pack(pack) {
            note.push_str(&format!("heavy={pack} (full flavor / CI stub)\n"));
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
    use progressive_lsp_engine::{discover_pack, is_pack_stub, PYTHON_PACK, RUST_PACK};

    #[test]
    fn dist_slim_default_excludes_heavy_packs() {
        let dir = tempfile::tempdir().unwrap();
        run(&["--dest".into(), dir.path().display().to_string()]).unwrap();
        let prefix = progressive_lsp_core::PrefixLayout::from_path(dir.path());
        assert!(discover_pack(&prefix, PYTHON_PACK).is_ok());
        assert!(discover_pack(&prefix, RUST_PACK).is_ok());
        assert!(discover_pack(&prefix, CLANGD_PACK).is_err());
        assert!(discover_pack(&prefix, TSGO_PACK).is_err());
        assert!(discover_pack(&prefix, GOPLS_PACK).is_err());
        assert!(discover_pack(&prefix, ZLS_PACK).is_err());
        let gap = std::fs::read_to_string(prefix.engines_dir().join("DARWIN_CI_GAP.txt")).unwrap();
        assert!(gap.contains("Slim default excludes"));
        assert!(is_pack_stub(
            &std::fs::read(discover_pack(&prefix, PYTHON_PACK).unwrap().path).unwrap()
        ));
    }

    #[test]
    fn dist_full_includes_heavy_stubs() {
        let dir = tempfile::tempdir().unwrap();
        run(&[
            "--pack".into(),
            "full".into(),
            "--dest".into(),
            dir.path().display().to_string(),
        ])
        .unwrap();
        let prefix = progressive_lsp_core::PrefixLayout::from_path(dir.path());
        for pack in [CLANGD_PACK, TSGO_PACK, GOPLS_PACK, ZLS_PACK] {
            let found = discover_pack(&prefix, pack).unwrap();
            assert!(is_pack_stub(&std::fs::read(&found.path).unwrap()));
        }
        assert!(run(&["--nope".into()]).is_err());
        assert!(run(&["--pack".into()]).is_err());
        assert!(run(&["--dest".into()]).is_err());
        assert!(run(&[
            "--pack".into(),
            "csharp-ls".into(),
            "--dest".into(),
            dir.path().display().to_string()
        ])
        .is_err());
        run(&[
            "--slim".into(),
            "--dest".into(),
            dir.path().display().to_string(),
        ])
        .unwrap();
        run(&[
            "--full".into(),
            "--dest".into(),
            dir.path().display().to_string(),
        ])
        .unwrap();
    }
}
