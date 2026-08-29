//! Host probe and build-file census value objects.

use std::collections::BTreeMap;
use std::path::Path;

use progressive_lsp_core::InstallError;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostProbe {
    pub arch: String,
    pub os: String,
    pub libc_hint: String,
    pub existing_hashes: BTreeMap<String, String>,
    pub census: BuildCensus,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildCensus {
    pub cargo_toml: bool,
    pub compile_commands: bool,
    pub cmake_lists: bool,
    pub pyproject_toml: bool,
    pub csproj: bool,
    pub tsconfig_json: bool,
    pub package_json: bool,
    pub composer_json: bool,
    pub go_mod: bool,
    pub go_work: bool,
    pub build_zig: bool,
    pub java_markers: bool,
}

impl BuildCensus {
    pub fn scan(root: &Path) -> Result<Self, InstallError> {
        if !root.is_dir() {
            return Err(InstallError::Io(format!(
                "census root is not a directory: {}",
                root.display()
            )));
        }
        Ok(Self {
            cargo_toml: root.join("Cargo.toml").is_file(),
            compile_commands: root.join("compile_commands.json").is_file(),
            cmake_lists: root.join("CMakeLists.txt").is_file(),
            pyproject_toml: root.join("pyproject.toml").is_file(),
            csproj: dir_has_suffix(root, ".csproj")?,
            tsconfig_json: root.join("tsconfig.json").is_file(),
            package_json: root.join("package.json").is_file(),
            composer_json: root.join("composer.json").is_file(),
            go_mod: root.join("go.mod").is_file(),
            go_work: root.join("go.work").is_file(),
            build_zig: root.join("build.zig").is_file(),
            java_markers: root.join("pom.xml").is_file()
                || root.join("build.gradle").is_file()
                || root.join("build.gradle.kts").is_file(),
        })
    }
}

impl HostProbe {
    pub fn current(census: BuildCensus) -> Self {
        Self {
            arch: std::env::consts::ARCH.to_string(),
            os: std::env::consts::OS.to_string(),
            libc_hint: libc_hint(),
            existing_hashes: BTreeMap::new(),
            census,
        }
    }
}

fn libc_hint() -> String {
    if cfg!(target_env = "musl") {
        "musl".into()
    } else if cfg!(target_env = "gnu") {
        "glibc".into()
    } else {
        "unknown".into()
    }
}

fn dir_has_suffix(root: &Path, suffix: &str) -> Result<bool, InstallError> {
    let entries = std::fs::read_dir(root)
        .map_err(|e| InstallError::Io(format!("read_dir {}: {e}", root.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| InstallError::Io(e.to_string()))?;
        if entry
            .file_name()
            .to_string_lossy()
            .ends_with(suffix)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_probe_has_arch_and_os() {
        let probe = HostProbe::current(BuildCensus::default());
        assert!(!probe.arch.is_empty());
        assert!(!probe.os.is_empty());
        assert!(!probe.libc_hint.is_empty());
        assert_ne!(probe, HostProbe::default());
        assert_ne!(probe.libc_hint, "");
        assert_ne!(probe.libc_hint, "xyzzy");
        assert!(probe.existing_hashes.is_empty());
        assert!(!probe.census.cargo_toml);
    }

    #[test]
    fn census_detects_manifests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(dir.path().join("pyproject.toml"), "").unwrap();
        std::fs::write(dir.path().join("App.csproj"), "").unwrap();
        std::fs::write(dir.path().join("go.mod"), "").unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();
        let c = BuildCensus::scan(dir.path()).unwrap();
        assert!(c.cargo_toml);
        assert!(c.pyproject_toml);
        assert!(c.csproj);
        assert!(c.go_mod);
        assert!(c.java_markers);
        assert!(!c.composer_json);
        assert!(!c.build_zig);
        assert!(!c.compile_commands);
        assert!(!c.cmake_lists);
        assert!(!c.tsconfig_json);
        assert!(!c.package_json);
        assert!(!c.go_work);

        let gradle = tempfile::tempdir().unwrap();
        std::fs::write(gradle.path().join("build.gradle"), "").unwrap();
        assert!(BuildCensus::scan(gradle.path()).unwrap().java_markers);
        let kts = tempfile::tempdir().unwrap();
        std::fs::write(kts.path().join("build.gradle.kts"), "").unwrap();
        assert!(BuildCensus::scan(kts.path()).unwrap().java_markers);
        let empty = tempfile::tempdir().unwrap();
        assert!(!BuildCensus::scan(empty.path()).unwrap().csproj);
        assert!(!BuildCensus::scan(empty.path()).unwrap().java_markers);
    }

    #[test]
    fn census_missing_root_is_error() {
        let err = BuildCensus::scan(Path::new("/no-such-census-root-plsp")).unwrap_err();
        assert!(matches!(err, InstallError::Io(_)));
    }

    #[test]
    fn libc_hint_is_known_token() {
        let h = libc_hint();
        assert!(h == "musl" || h == "glibc" || h == "unknown");
    }
}
