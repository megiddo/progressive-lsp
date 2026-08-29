//! WatchFilter decorator. Dropped paths never enter DirtySet.

use crate::WatchBatch;

/// Default ignore globs (path fragments). Manifests are still watched.
pub const DEFAULT_IGNORE_GLOBS: &[&str] = &[
    "/node_modules/",
    "/.git/objects/",
    "/vendor/",
    "/zig-cache/",
    "/.zig-cache/",
];

/// Manifest file names that must not be dropped even under an ignore parent.
pub const MANIFEST_NAMES: &[&str] = &[
    "Cargo.toml",
    "pom.xml",
    "composer.json",
    "tsconfig.json",
    "compile_commands.json",
    "go.mod",
    "go.work",
    "build.zig",
    "build.zig.zon",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    ".classpath",
    ".project",
];

pub trait WatchFilter: Send + Sync {
    fn filter(&self, batch: WatchBatch) -> WatchBatch;
}

/// Pass-through. Valid v1 default for tests that do not want ignore globs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdentityWatchFilter;

impl WatchFilter for IdentityWatchFilter {
    fn filter(&self, batch: WatchBatch) -> WatchBatch {
        batch
    }
}

/// Drops ignored vendor/object-store paths; keeps manifests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultIgnoreFilter;

impl DefaultIgnoreFilter {
    pub fn should_drop(path: &str) -> bool {
        let normalized = path.replace('\\', "/");
        if is_manifest(&normalized) {
            return false;
        }
        DEFAULT_IGNORE_GLOBS.iter().any(|g| {
            normalized.contains(g) || normalized.starts_with(g.trim_start_matches('/'))
        })
    }
}

impl WatchFilter for DefaultIgnoreFilter {
    fn filter(&self, mut batch: WatchBatch) -> WatchBatch {
        batch.events.retain(|e| !Self::should_drop(&e.path));
        batch
    }
}

fn is_manifest(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if MANIFEST_NAMES.iter().any(|m| *m == name) {
        return true;
    }
    name.ends_with(".csproj")
}

/// Drop an explicit path list. Used to prove dropped paths never enter DirtySet.
#[derive(Clone, Debug, Default)]
pub struct DenyListFilter {
    pub denied: Vec<String>,
}

impl WatchFilter for DenyListFilter {
    fn filter(&self, mut batch: WatchBatch) -> WatchBatch {
        batch
            .events
            .retain(|e| !self.denied.iter().any(|d| e.path == *d));
        batch
    }
}

pub fn apply_filter(filter: &dyn WatchFilter, batch: WatchBatch) -> WatchBatch {
    filter.filter(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::WatchKind;
    use crate::WatchEvent;

    fn ev(path: &str) -> WatchEvent {
        WatchEvent::new(path, WatchKind::Modify)
    }

    fn batch(paths: &[&str]) -> WatchBatch {
        WatchBatch {
            events: paths.iter().map(|p| ev(p)).collect(),
            overflow: false,
            need_rescan: false,
            generation: 1,
        }
    }

    #[test]
    fn identity_keeps_every_path() {
        let raw = batch(&["a.java", "node_modules/x.js"]);
        let out = IdentityWatchFilter.filter(raw.clone());
        assert_eq!(out, raw);
        assert_eq!(apply_filter(&IdentityWatchFilter, raw.clone()).events.len(), 2);
    }

    #[test]
    fn ignore_drops_vendor_and_keeps_manifests() {
        let raw = batch(&[
            "src/A.java",
            "node_modules/left-pad/index.js",
            "pkg/node_modules/foo/bar.js",
            "repo/.git/objects/aa/bb",
            "third/vendor/pkg/x.c",
            "z/zig-cache/o",
            "z/.zig-cache/o",
            "pkg/node_modules/foo/package.json",
            "app/pom.xml",
            "app/Cargo.toml",
            "app/composer.json",
            "app/tsconfig.json",
            "app/compile_commands.json",
            "app/go.mod",
            "app/go.work",
            "app/build.zig",
            "app/build.zig.zon",
            "app/build.gradle",
            "app/settings.gradle",
            "app/Lib.csproj",
            "app/.classpath",
            "app/.project",
            r"win\node_modules\x.js",
        ]);
        let out = DefaultIgnoreFilter.filter(raw);
        let paths: Vec<_> = out.events.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"src/A.java"));
        assert!(paths.contains(&"app/pom.xml"));
        assert!(paths.contains(&"app/Cargo.toml"));
        assert!(paths.contains(&"app/Lib.csproj"));
        assert!(paths.contains(&"app/.classpath"));
        assert!(!paths.iter().any(|p| p.contains("left-pad")));
        assert!(!paths.iter().any(|p| p.contains(".git/objects")));
        assert!(!paths.iter().any(|p| p.contains("/vendor/")));
        assert!(!paths.iter().any(|p| p.contains("zig-cache")));
        assert!(!DefaultIgnoreFilter::should_drop("src/A.java"));
        assert!(DefaultIgnoreFilter::should_drop("x/node_modules/y.js"));
        assert!(!DefaultIgnoreFilter::should_drop("x/node_modules/y/pom.xml"));
        assert!(!is_manifest("src/A.java"));
        assert!(is_manifest("Foo.csproj"));
    }

    #[test]
    fn deny_list_drops_exact_paths() {
        let filter = DenyListFilter {
            denied: vec!["secret.java".into()],
        };
        let out = filter.filter(batch(&["secret.java", "ok.java"]));
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0].path, "ok.java");
    }
}
