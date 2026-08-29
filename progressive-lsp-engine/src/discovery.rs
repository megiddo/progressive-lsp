//! Pack discovery under `$PREFIX/engines/`. Missing or bad hash → no spawn.

use progressive_lsp_core::{EngineError, PrefixLayout};
use progressive_lsp_install::{hex_encode, sha256_file, verify_hash, Manifest};

use crate::adapter::EngineBinary;

pub const PYTHON_PACK: &str = "python";
pub const RUST_PACK: &str = "rust";
pub const CLANGD_PACK: &str = "clangd";
pub const TSGO_PACK: &str = "tsgo";
pub const PHPANTOM_PACK: &str = "phpantom";
pub const SUPERHTML_PACK: &str = "superhtml";
pub const BIOME_PACK: &str = "biome";
pub const GOPLS_PACK: &str = "gopls";
pub const ZLS_PACK: &str = "zls";

pub const TY_BINARY: &str = "ty";
pub const RA_BINARY: &str = "rust-analyzer";
pub const CLANGD_BINARY: &str = "clangd";
pub const TSGO_BINARY: &str = "tsgo";
pub const PHPANTOM_BINARY: &str = "phpantom";
pub const SUPERHTML_BINARY: &str = "superhtml";
pub const BIOME_BINARY: &str = "biome";
pub const GOPLS_BINARY: &str = "gopls";
pub const ZLS_BINARY: &str = "zls";

/// Slim default: Java-only / light workspaces. Excludes clangd, tsgo, gopls, zls.
pub fn slim_pack_names() -> &'static [&'static str] {
    &[
        PYTHON_PACK,
        RUST_PACK,
        PHPANTOM_PACK,
        SUPERHTML_PACK,
        BIOME_PACK,
    ]
}

/// Full CI flavor: slim plus heavy C/C++/TS/Go/Zig packs (stubs on Darwin).
pub fn full_pack_names() -> &'static [&'static str] {
    &[
        PYTHON_PACK,
        RUST_PACK,
        PHPANTOM_PACK,
        SUPERHTML_PACK,
        BIOME_PACK,
        CLANGD_PACK,
        TSGO_PACK,
        GOPLS_PACK,
        ZLS_PACK,
    ]
}

pub fn is_heavy_pack(pack_name: &str) -> bool {
    matches!(
        pack_name,
        CLANGD_PACK | TSGO_PACK | GOPLS_PACK | ZLS_PACK
    )
}

pub fn pack_dir(prefix: &PrefixLayout, pack_name: &str) -> std::path::PathBuf {
    prefix.engines_dir().join(pack_name)
}

pub fn binary_name_for_pack(pack_name: &str) -> Option<&'static str> {
    match pack_name {
        PYTHON_PACK => Some(TY_BINARY),
        RUST_PACK => Some(RA_BINARY),
        CLANGD_PACK => Some(CLANGD_BINARY),
        TSGO_PACK => Some(TSGO_BINARY),
        PHPANTOM_PACK => Some(PHPANTOM_BINARY),
        SUPERHTML_PACK => Some(SUPERHTML_BINARY),
        BIOME_PACK => Some(BIOME_BINARY),
        GOPLS_PACK => Some(GOPLS_BINARY),
        ZLS_PACK => Some(ZLS_BINARY),
        _ => None,
    }
}

/// Discover `<engines>/<pack>/<binary>` + `manifest.json` and verify sha256.
pub fn discover_pack(prefix: &PrefixLayout, pack_name: &str) -> Result<EngineBinary, EngineError> {
    let binary_name = binary_name_for_pack(pack_name)
        .ok_or_else(|| EngineError::NotDiscovered(pack_name.into()))?;
    let dir = pack_dir(prefix, pack_name);
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(EngineError::NotDiscovered(format!(
            "{pack_name}: missing {}",
            manifest_path.display()
        )));
    }
    let json = std::fs::read_to_string(&manifest_path)
        .map_err(|e| EngineError::NotDiscovered(format!("read manifest: {e}")))?;
    let manifest = Manifest::parse(&json)
        .map_err(|e| EngineError::NotDiscovered(format!("manifest: {e}")))?;
    let art = manifest
        .artifacts
        .iter()
        .find(|a| a.name == binary_name || a.rel_path == binary_name)
        .ok_or_else(|| EngineError::NotDiscovered(format!("{pack_name}: no {binary_name} artifact")))?;
    let path = dir.join(&art.rel_path);
    if !path.is_file() {
        return Err(EngineError::NotDiscovered(format!(
            "{pack_name}: missing {}",
            path.display()
        )));
    }
    let actual = sha256_file(&path).map_err(|e| EngineError::NotDiscovered(e.to_string()))?;
    verify_hash(&actual, &art.sha256).map_err(|e| match e {
        progressive_lsp_core::InstallError::Hash { expected, actual } => {
            EngineError::Hash { expected, actual }
        }
        other => EngineError::NotDiscovered(other.to_string()),
    })?;
    Ok(EngineBinary {
        pack_name: pack_name.to_string(),
        path,
        sha256: actual,
    })
}

pub fn discover_pack_opt(prefix: &PrefixLayout, pack_name: &str) -> Option<EngineBinary> {
    discover_pack(prefix, pack_name).ok()
}

pub fn stub_pack_bytes(pack_name: &str, binary_name: &str) -> Vec<u8> {
    format!(
        "progressive-lsp-pack-stub:{binary_name}\n\
         pack={pack_name}\n\
         # Not a musl ELF. Real engine static packs are Linux CI / Docker.\n"
    )
    .into_bytes()
}

pub fn is_pack_stub(bytes: &[u8]) -> bool {
    bytes.starts_with(b"progressive-lsp-pack-stub:")
}

pub fn hex_of(bytes: &[u8]) -> String {
    hex_encode(&progressive_lsp_install::sha256(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_install::{hex_encode, sha256, Manifest, ManifestArtifact};

    fn write_pack(prefix: &PrefixLayout, pack: &str, binary: &str, bytes: &[u8], sha: &str) {
        let dir = pack_dir(prefix, pack);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(binary), bytes).unwrap();
        let m = Manifest {
            version: "1".into(),
            artifacts: vec![ManifestArtifact {
                name: binary.into(),
                rel_path: binary.into(),
                sha256: sha.to_string(),
                executable: true,
            }],
        };
        std::fs::write(dir.join("manifest.json"), m.to_json().unwrap()).unwrap();
    }

    #[test]
    fn missing_pack_is_not_discovered() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path());
        prefix.ensure_dirs().unwrap();
        assert!(discover_pack_opt(&prefix, PYTHON_PACK).is_none());
        let err = discover_pack(&prefix, PYTHON_PACK).unwrap_err();
        assert!(matches!(err, EngineError::NotDiscovered(_)));
        assert!(discover_pack(&prefix, "csharp-ls").is_err());
        assert_eq!(binary_name_for_pack(PYTHON_PACK), Some(TY_BINARY));
        assert_eq!(binary_name_for_pack(RUST_PACK), Some(RA_BINARY));
        assert_eq!(binary_name_for_pack(CLANGD_PACK), Some(CLANGD_BINARY));
        assert_eq!(binary_name_for_pack(TSGO_PACK), Some(TSGO_BINARY));
        assert_eq!(binary_name_for_pack(PHPANTOM_PACK), Some(PHPANTOM_BINARY));
        assert_eq!(binary_name_for_pack(SUPERHTML_PACK), Some(SUPERHTML_BINARY));
        assert_eq!(binary_name_for_pack(BIOME_PACK), Some(BIOME_BINARY));
        assert_eq!(binary_name_for_pack(GOPLS_PACK), Some(GOPLS_BINARY));
        assert_eq!(binary_name_for_pack(ZLS_PACK), Some(ZLS_BINARY));
        assert!(binary_name_for_pack("csharp-ls").is_none());
        assert_eq!(pack_dir(&prefix, "python"), prefix.engines_dir().join("python"));
        assert!(slim_pack_names().contains(&PYTHON_PACK));
        assert!(!slim_pack_names().contains(&CLANGD_PACK));
        assert!(full_pack_names().contains(&CLANGD_PACK));
        assert!(is_heavy_pack(CLANGD_PACK));
        assert!(!is_heavy_pack(PYTHON_PACK));
    }

    #[test]
    fn good_hash_discovers_and_bad_hash_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path());
        prefix.ensure_dirs().unwrap();
        let bytes = stub_pack_bytes(PYTHON_PACK, TY_BINARY);
        assert!(is_pack_stub(&bytes));
        assert!(!is_pack_stub(b"ELF"));
        let sha = hex_encode(&sha256(&bytes));
        write_pack(&prefix, PYTHON_PACK, TY_BINARY, &bytes, &sha);
        let found = discover_pack(&prefix, PYTHON_PACK).unwrap();
        assert_eq!(found.pack_name, PYTHON_PACK);
        assert_eq!(found.path, pack_dir(&prefix, PYTHON_PACK).join(TY_BINARY));
        assert_eq!(hex_of(&bytes), sha);

        write_pack(
            &prefix,
            RUST_PACK,
            RA_BINARY,
            b"other",
            &hex_encode(&sha256(b"not-other")),
        );
        let err = discover_pack(&prefix, RUST_PACK).unwrap_err();
        assert!(matches!(err, EngineError::Hash { .. }), "{err:?}");
    }

    #[test]
    fn missing_binary_or_bad_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = PrefixLayout::from_path(dir.path());
        let d = pack_dir(&prefix, PYTHON_PACK);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("manifest.json"), "{}").unwrap();
        assert!(discover_pack(&prefix, PYTHON_PACK).is_err());
        let bytes = b"x";
        write_pack(
            &prefix,
            PYTHON_PACK,
            "not-ty",
            bytes,
            &hex_encode(&sha256(bytes)),
        );
        assert!(discover_pack(&prefix, PYTHON_PACK).is_err());
        write_pack(
            &prefix,
            PYTHON_PACK,
            TY_BINARY,
            bytes,
            &hex_encode(&sha256(bytes)),
        );
        std::fs::remove_file(d.join(TY_BINARY)).unwrap();
        assert!(discover_pack(&prefix, PYTHON_PACK).is_err());
    }
}
