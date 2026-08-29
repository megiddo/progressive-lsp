//! `xtask dist --pack slim|full|python,rust,...`: per-triple tarballs + SHA256 + manifest.
//!
//! On Darwin this host writes tarball **layout** + SHA256 + `manifest.json` with pack **stubs**.
//! Those stubs are not musl ELFs. Do not run `check-static` on them or claim greens.
//! Real per-triple musl tarballs (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`)
//! are Linux CI / Docker. Slim vs full matches M4 (slim excludes clangd/tsgo/gopls/zls).
//! Allocator: this command **only** reads `xtask/allocator-matrix.toml`.

use std::path::{Path, PathBuf};

use progressive_lsp_engine::{
    binary_name_for_pack, full_pack_names, hex_of, is_heavy_pack, pack_dir, slim_pack_names,
    stub_pack_bytes, CLANGD_PACK, GOPLS_PACK, TSGO_PACK, ZLS_PACK,
};
use progressive_lsp_install::{
    hex_encode, sha256, DistArtifact, DistManifest, Manifest, ManifestArtifact, DIST_PAYLOAD_STUB,
    DIST_PROTO, MUSL_TRIPLES,
};

use crate::tarball::{collect_dir, sha256_sidecar, write_tar_file};

pub fn run(args: &[String]) -> Result<(), String> {
    let mut packs: Vec<String> = Vec::new();
    let mut dest: Option<PathBuf> = None;
    let mut libc = "musl";
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
            "--libc" => {
                i += 1;
                libc = args
                    .get(i)
                    .ok_or("--libc requires musl or glibc-static")?
                    .as_str();
                if libc != "musl" && libc != "glibc-static" {
                    return Err(format!("unknown --libc {libc} (musl|glibc-static)"));
                }
            }
            other => return Err(format!("unknown dist flag: {other}")),
        }
        i += 1;
    }
    if packs.is_empty() {
        packs = slim_pack_names().iter().map(|s| (*s).to_string()).collect();
    }
    let dest = dest.ok_or("dist requires --dest DIR")?;
    let matrix = read_allocator_matrix()?;
    write_packs(&dest, &packs)?;
    write_per_triple_tarballs(&dest, &packs, libc, &matrix)
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

fn flavor_of(packs: &[String]) -> &'static str {
    let slim: Vec<String> = slim_pack_names().iter().map(|s| (*s).to_string()).collect();
    let full: Vec<String> = full_pack_names().iter().map(|s| (*s).to_string()).collect();
    if packs == full.as_slice() {
        "full"
    } else if packs == slim.as_slice() {
        "slim"
    } else {
        "custom"
    }
}

pub fn write_packs(dest: &Path, packs: &[String]) -> Result<(), String> {
    let prefix = progressive_lsp_core::PrefixLayout::from_path(dest);
    prefix
        .ensure_dirs()
        .map_err(|e| format!("ensure prefix: {e}"))?;
    let mut note = String::from(dist_readme());
    for pack in packs {
        let binary = binary_name_for_pack(pack)
            .ok_or_else(|| format!("unknown pack {pack}; known: slim, full, or named packs"))?;
        write_one(&prefix, pack, binary)?;
        note.push_str(&format!("pack={pack} binary={binary}\n"));
        if is_heavy_pack(pack) {
            note.push_str(&format!("heavy={pack} (full flavor / CI stub)\n"));
        }
    }
    std::fs::write(prefix.engines_dir().join("DARWIN_CI_GAP.txt"), &note)
        .map_err(|e| format!("write gap note: {e}"))?;
    std::fs::write(dest.join("DIST_README.txt"), dist_readme())
        .map_err(|e| format!("write DIST_README: {e}"))?;
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

fn write_per_triple_tarballs(
    dest: &Path,
    packs: &[String],
    libc: &str,
    matrix: &str,
) -> Result<(), String> {
    let flavor = flavor_of(packs);
    let triples: &[&str] = match libc {
        "glibc-static" => &[
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
        ],
        _ => MUSL_TRIPLES,
    };
    let mut files = collect_dir(dest, "")?;
    files.retain(|(n, _)| n != "DIST_README.txt" && !n.ends_with(".tar") && !n.ends_with(".sha256"));
    files.push(("DIST_README.txt".into(), dist_readme().as_bytes().to_vec()));
    files.push(("allocator-matrix.toml".into(), matrix.as_bytes().to_vec()));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut dist = DistManifest::new(env!("CARGO_PKG_VERSION"), DIST_PAYLOAD_STUB);
    for triple in triples {
        let tar_rel = format!("{triple}/{flavor}.tar");
        let tar_path = dest.join(&tar_rel);
        write_tar_file(&tar_path, &files)?;
        let bytes = std::fs::read(&tar_path).map_err(|e| format!("read tar: {e}"))?;
        let hex = hex_encode(&sha256(&bytes));
        sha256_sidecar(&tar_path, &hex)?;
        dist.artifacts.push(DistArtifact {
            triple: (*triple).into(),
            flavor: flavor.into(),
            rel_path: tar_rel,
            sha256: hex,
        });
    }
    std::fs::write(dest.join("manifest.json"), dist.to_json().map_err(|e| e.to_string())?)
        .map_err(|e| format!("write dist manifest: {e}"))?;
    let _ = DIST_PROTO;
    Ok(())
}

fn dist_readme() -> &'static str {
    "progressive-lsp dist (v1)\n\
     \n\
     THIS HOST (typically Darwin) writes tarball LAYOUT + SHA256 + manifest.json.\n\
     Pack payloads here are STUBS, not musl ELFs. Do not treat them as check-static greens.\n\
     \n\
     The real dist is Linux CI per-triple musl tarballs:\n\
       x86_64-unknown-linux-musl/<flavor>.tar\n\
       aarch64-unknown-linux-musl/<flavor>.tar\n\
     Those CI artifacts must pass `xtask check-static` (no interpreter, no DT_NEEDED).\n\
     \n\
     Slim default excludes clangd, tsgo, gopls, zls (M4).\n\
     Core crate semver is independent of engine SHAs in each pack manifest.json.\n\
     Proto stays progressive.v1.\n\
     Allocator: xtask dist only reads xtask/allocator-matrix.toml.\n"
}

fn read_allocator_matrix() -> Result<String, String> {
    let path = crate::allocator::matrix_path();
    std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "dist only reads {}: {e}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use progressive_lsp_engine::{discover_pack, is_pack_stub, PYTHON_PACK, RUST_PACK};
    use progressive_lsp_install::DistManifest;

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
    fn dist_writes_per_triple_tarballs_and_sha256() {
        let dir = tempfile::tempdir().unwrap();
        run(&["--dest".into(), dir.path().display().to_string()]).unwrap();
        let json = std::fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        let manifest = DistManifest::parse(&json).unwrap();
        assert_eq!(manifest.core_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(manifest.proto, DIST_PROTO);
        assert_eq!(manifest.payload_kind, DIST_PAYLOAD_STUB);
        assert_ne!(manifest.payload_kind, "musl-elf");
        assert_eq!(manifest.artifacts.len(), 2);
        for art in &manifest.artifacts {
            assert!(MUSL_TRIPLES.contains(&art.triple.as_str()));
            assert_eq!(art.flavor, "slim");
            let tar = dir.path().join(&art.rel_path);
            assert!(tar.is_file(), "{}", tar.display());
            let bytes = std::fs::read(&tar).unwrap();
            assert_eq!(hex_encode(&sha256(&bytes)), art.sha256);
            let side = format!("{}.sha256", tar.display());
            let side_txt = std::fs::read_to_string(&side).unwrap();
            assert!(side_txt.starts_with(&art.sha256));
            let tar_txt = std::str::from_utf8(&bytes).unwrap_or("");
            assert!(!tar_txt.contains("check-static: PASS"));
            assert!(tar_txt.contains("STUBS") || tar_txt.contains("stub"));
        }
        let readme = std::fs::read_to_string(dir.path().join("DIST_README.txt")).unwrap();
        assert!(readme.contains("STUBS"));
        assert!(readme.contains("Linux CI"));
        assert!(readme.contains("x86_64-unknown-linux-musl"));
        assert!(readme.contains("not musl ELFs") || readme.contains("not musl ELF"));
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
        let json = std::fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        let manifest = DistManifest::parse(&json).unwrap();
        assert!(manifest.artifacts.iter().all(|a| a.flavor == "full"));
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
        run(&[
            "--libc".into(),
            "musl".into(),
            "--dest".into(),
            dir.path().display().to_string(),
        ])
        .unwrap();
        assert!(run(&[
            "--libc".into(),
            "weird".into(),
            "--dest".into(),
            dir.path().display().to_string()
        ])
        .is_err());
        assert!(run(&["--libc".into()]).is_err());
        let gnu = tempfile::tempdir().unwrap();
        run(&[
            "--libc".into(),
            "glibc-static".into(),
            "--dest".into(),
            gnu.path().display().to_string(),
        ])
        .unwrap();
        assert!(gnu
            .path()
            .join("x86_64-unknown-linux-gnu/slim.tar")
            .is_file());
        run(&[
            "--pack".into(),
            "python".into(),
            "--dest".into(),
            dir.path().display().to_string(),
        ])
        .unwrap();
        let custom = DistManifest::parse(
            &std::fs::read_to_string(dir.path().join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert!(custom.artifacts.iter().all(|a| a.flavor == "custom"));
    }

    #[test]
    fn dist_reads_allocator_matrix_only() {
        let text = read_allocator_matrix().unwrap();
        assert!(text.contains("mimalloc"));
        assert!(text.contains("[[cell]]"));
    }
}
