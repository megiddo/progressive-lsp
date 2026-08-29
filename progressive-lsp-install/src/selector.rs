//! `PackSelector` strategies: explicit list vs census.

use crate::probe::HostProbe;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackId(pub String);

impl PackId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub trait PackSelector {
    fn select(&self, probe: &HostProbe) -> Vec<PackId>;
}

/// Explicit `--packs` list. Order is preserved; empty tokens dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplicitPacks {
    packs: Vec<PackId>,
}

impl ExplicitPacks {
    pub fn new(packs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let packs = packs
            .into_iter()
            .map(Into::into)
            .filter(|s| !s.is_empty())
            .map(PackId::new)
            .collect();
        Self { packs }
    }

    pub fn parse_csv(csv: &str) -> Self {
        Self::new(csv.split(',').map(str::trim))
    }
}

impl PackSelector for ExplicitPacks {
    fn select(&self, _probe: &HostProbe) -> Vec<PackId> {
        self.packs.clone()
    }
}

/// Census → packs (auto). Java has no T3 pack.
#[derive(Clone, Debug, Default)]
pub struct CensusSelector;

impl PackSelector for CensusSelector {
    fn select(&self, probe: &HostProbe) -> Vec<PackId> {
        let c = &probe.census;
        let mut packs = Vec::new();
        if c.cargo_toml {
            packs.push(PackId::new("rust-analyzer"));
        }
        if c.compile_commands || c.cmake_lists {
            packs.push(PackId::new("clangd"));
        }
        if c.pyproject_toml {
            packs.push(PackId::new("ty"));
        }
        if c.csproj {
            packs.push(PackId::new("csharp-ls"));
        }
        if c.tsconfig_json || c.package_json {
            packs.push(PackId::new("tsgo"));
        }
        if c.composer_json {
            packs.push(PackId::new("phpantom"));
        }
        if c.go_mod || c.go_work {
            packs.push(PackId::new("gopls"));
        }
        if c.build_zig {
            packs.push(PackId::new("zls"));
        }
        let _ = c.java_markers;
        packs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{BuildCensus, HostProbe};

    fn probe_with(mut f: impl FnMut(&mut BuildCensus)) -> HostProbe {
        let mut c = BuildCensus::default();
        f(&mut c);
        HostProbe::current(c)
    }

    #[test]
    fn explicit_csv_and_empty_tokens() {
        let sel = ExplicitPacks::parse_csv("python, rust,");
        let packs = sel.select(&probe_with(|_| {}));
        assert_eq!(
            packs,
            vec![PackId::new("python"), PackId::new("rust")]
        );
        assert_eq!(packs[0].as_str(), "python");
        assert_ne!(packs[0].as_str(), "");
        assert!(ExplicitPacks::parse_csv("").select(&probe_with(|_| {})).is_empty());
    }

    #[test]
    fn census_maps_each_manifest() {
        let cases: &[(&str, fn(&mut BuildCensus), &str)] = &[
            ("cargo", |c| c.cargo_toml = true, "rust-analyzer"),
            ("cc", |c| c.compile_commands = true, "clangd"),
            ("cmake", |c| c.cmake_lists = true, "clangd"),
            ("py", |c| c.pyproject_toml = true, "ty"),
            ("cs", |c| c.csproj = true, "csharp-ls"),
            ("ts", |c| c.tsconfig_json = true, "tsgo"),
            ("pkg", |c| c.package_json = true, "tsgo"),
            ("php", |c| c.composer_json = true, "phpantom"),
            ("go", |c| c.go_mod = true, "gopls"),
            ("gowork", |c| c.go_work = true, "gopls"),
            ("zig", |c| c.build_zig = true, "zls"),
        ];
        for (name, set, pack) in cases {
            let got = CensusSelector.select(&probe_with(set));
            assert_eq!(got, vec![PackId::new(*pack)], "{name}");
        }
        let java_only = CensusSelector.select(&probe_with(|c| c.java_markers = true));
        assert!(java_only.is_empty(), "Java has no T3 pack");
    }

    #[test]
    fn census_can_select_several() {
        let packs = CensusSelector.select(&probe_with(|c| {
            c.cargo_toml = true;
            c.pyproject_toml = true;
        }));
        assert_eq!(
            packs,
            vec![PackId::new("rust-analyzer"), PackId::new("ty")]
        );
    }
}
