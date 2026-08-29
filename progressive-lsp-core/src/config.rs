//! `config.toml` schema. Empty config is valid; unknown keys warn.

use std::collections::BTreeMap;

use crate::error::ConfigError;

/// Written when [`crate::PrefixLayout::ensure_dirs`] creates a missing file.
pub const EMPTY_CONFIG_TOML: &str = "# progressive-lsp config stub (M0)\n# All keys optional. Empty file is valid.\n# [t2] java = \"heuristic\"  # default; stack-graphs is opt-in\n";

/// Per-language T2 Strategy pick. Omit = [`T2Backend::Heuristic`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum T2Backend {
    #[default]
    Heuristic,
    StackGraphs,
}

impl T2Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Heuristic => "heuristic",
            Self::StackGraphs => "stack-graphs",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "heuristic" => Some(Self::Heuristic),
            "stack-graphs" => Some(Self::StackGraphs),
            _ => None,
        }
    }
}

/// Per-language T2 picks. Missing language → heuristic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct T2Table {
    pub picks: BTreeMap<String, T2Backend>,
}

impl T2Table {
    pub fn get(&self, language: &str) -> T2Backend {
        self.picks
            .get(language)
            .copied()
            .unwrap_or(T2Backend::Heuristic)
    }

    pub fn is_empty(&self) -> bool {
        self.picks.is_empty()
    }
}

/// Merged config. Required keys: none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub packs: Vec<String>,
    pub scripts: Vec<String>,
    pub prefix: Option<String>,
    pub t2: T2Table,
}

impl Config {
    pub fn empty() -> Self {
        Self {
            packs: Vec::new(),
            scripts: Vec::new(),
            prefix: None,
            t2: T2Table::default(),
        }
    }

    pub fn t2_for(&self, language: &str) -> T2Backend {
        self.t2.get(language)
    }

    pub fn from_toml(src: &str) -> Result<ConfigLoad, ConfigError> {
        ConfigOverlay::parse(src).map(|overlay| ConfigLoad {
            config: Config::empty().merge(&overlay),
            warnings: overlay.warnings.clone(),
        })
    }

    /// Snapshot for `GetConfig`. Only known keys; unknown keys are never invented.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        if !self.packs.is_empty() {
            out.push_str("packs = [");
            for (i, p) in self.packs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("\"{}\"", escape_toml_string(p)));
            }
            out.push_str("]\n");
        }
        if !self.scripts.is_empty() {
            out.push_str("scripts = [");
            for (i, p) in self.scripts.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("\"{}\"", escape_toml_string(p)));
            }
            out.push_str("]\n");
        }
        if let Some(prefix) = &self.prefix {
            out.push_str(&format!("prefix = \"{}\"\n", escape_toml_string(prefix)));
        }
        if !self.t2.is_empty() {
            out.push_str("[t2]\n");
            for (lang, backend) in &self.t2.picks {
                out.push_str(&format!(
                    "{} = \"{}\"\n",
                    lang,
                    escape_toml_string(backend.as_str())
                ));
            }
        }
        out
    }

    /// Overlay wins for keys it **sets**. Unset keys fall through.
    /// `[t2]` merges per language: overlay keys win; others remain.
    pub fn merge(&self, overlay: &ConfigOverlay) -> Self {
        let t2 = match &overlay.t2 {
            Some(over) => {
                let mut picks = self.t2.picks.clone();
                for (lang, backend) in &over.picks {
                    picks.insert(lang.clone(), *backend);
                }
                T2Table { picks }
            }
            None => self.t2.clone(),
        };
        Self {
            packs: overlay.packs.clone().unwrap_or_else(|| self.packs.clone()),
            scripts: overlay.scripts.clone().unwrap_or_else(|| self.scripts.clone()),
            prefix: overlay.prefix.clone().or_else(|| self.prefix.clone()),
            t2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigLoad {
    pub config: Config,
    pub warnings: Vec<String>,
}

/// Keys present in one TOML file (for merge-chain "later wins if set").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigOverlay {
    pub packs: Option<Vec<String>>,
    pub scripts: Option<Vec<String>>,
    pub prefix: Option<String>,
    pub t2: Option<T2Table>,
    pub warnings: Vec<String>,
}

impl ConfigOverlay {
    pub fn parse(src: &str) -> Result<Self, ConfigError> {
        if src.trim().is_empty() {
            return Ok(Self::default());
        }
        let table: toml::Table = src
            .parse()
            .map_err(|e: toml::de::Error| ConfigError::Toml(e.to_string()))?;
        let mut overlay = Self::default();
        for (key, value) in &table {
            match key.as_str() {
                "packs" => {
                    overlay.packs = Some(string_array(value, "packs")?);
                }
                "scripts" => {
                    overlay.scripts = Some(string_array(value, "scripts")?);
                }
                "prefix" => {
                    overlay.prefix = Some(string_value(value, "prefix")?);
                }
                "t2" => {
                    overlay.t2 = Some(parse_t2_table(value, &mut overlay.warnings)?);
                }
                other => overlay
                    .warnings
                    .push(format!("unknown config key ignored: {other}")),
            }
        }
        Ok(overlay)
    }
}

fn parse_t2_table(value: &toml::Value, warnings: &mut Vec<String>) -> Result<T2Table, ConfigError> {
    let table = value
        .as_table()
        .ok_or_else(|| ConfigError::Toml("t2 must be a table of language = backend".into()))?;
    let mut picks = BTreeMap::new();
    for (lang, backend) in table {
        let raw = backend
            .as_str()
            .ok_or_else(|| ConfigError::Toml(format!("t2.{lang} must be a string")))?;
        match T2Backend::parse(raw) {
            Some(pick) => {
                picks.insert(lang.clone(), pick);
            }
            None => warnings.push(format!("unknown t2 backend ignored: {lang}={raw}")),
        }
    }
    Ok(T2Table { picks })
}

fn string_array(value: &toml::Value, key: &str) -> Result<Vec<String>, ConfigError> {
    let arr = value
        .as_array()
        .ok_or_else(|| ConfigError::Toml(format!("{key} must be an array of strings")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item.as_str().ok_or_else(|| {
            ConfigError::Toml(format!("{key} must be an array of strings"))
        })?;
        out.push(s.to_string());
    }
    Ok(out)
}

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn string_value(value: &toml::Value, key: &str) -> Result<String, ConfigError> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ConfigError::Toml(format!("{key} must be a string")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_valid() {
        let load = Config::from_toml("").unwrap();
        assert_eq!(load.config, Config::empty());
        assert!(load.warnings.is_empty());
        let load = Config::from_toml("   \n# comment\n").unwrap();
        assert_eq!(load.config, Config::empty());
        assert_eq!(load.config.t2_for("java"), T2Backend::Heuristic);
    }

    #[test]
    fn known_keys_parse() {
        let load = Config::from_toml(
            r#"
packs = ["python", "rust"]
scripts = ["deny.rhai"]
prefix = "/tmp/x"
"#,
        )
        .unwrap();
        assert_eq!(load.config.packs, ["python", "rust"]);
        assert_eq!(load.config.scripts, ["deny.rhai"]);
        assert_eq!(load.config.prefix.as_deref(), Some("/tmp/x"));
        assert!(load.warnings.is_empty());
        assert_eq!(load.config.t2_for("java"), T2Backend::Heuristic);
    }

    #[test]
    fn t2_table_defaults_to_heuristic_and_parses_opt_in() {
        assert_eq!(T2Backend::default().as_str(), "heuristic");
        assert_eq!(T2Backend::Heuristic.as_str(), "heuristic");
        assert_eq!(T2Backend::StackGraphs.as_str(), "stack-graphs");
        assert_eq!(T2Backend::parse("heuristic"), Some(T2Backend::Heuristic));
        assert_eq!(T2Backend::parse("stack-graphs"), Some(T2Backend::StackGraphs));
        assert_eq!(T2Backend::parse("oxc"), None);
        let omitted = Config::from_toml("[t2]\n").unwrap();
        assert_eq!(omitted.config.t2_for("java"), T2Backend::Heuristic);
        assert!(omitted.config.t2.is_empty());
        let load = Config::from_toml(
            r#"
[t2]
java = "stack-graphs"
php = "heuristic"
"#,
        )
        .unwrap();
        assert_eq!(load.config.t2_for("java"), T2Backend::StackGraphs);
        assert_eq!(load.config.t2_for("php"), T2Backend::Heuristic);
        assert_eq!(load.config.t2_for("go"), T2Backend::Heuristic);
        assert!(load.warnings.is_empty());
        let snap = load.config.to_toml();
        assert!(snap.contains("[t2]"));
        assert!(snap.contains("java = \"stack-graphs\""));
        let round = Config::from_toml(&snap).unwrap();
        assert_eq!(round.config.t2, load.config.t2);
    }

    #[test]
    fn unknown_t2_backend_warns_and_stays_heuristic() {
        let load = Config::from_toml("[t2]\njava = \"magic\"\n").unwrap();
        assert_eq!(load.config.t2_for("java"), T2Backend::Heuristic);
        assert_eq!(
            load.warnings,
            ["unknown t2 backend ignored: java=magic"]
        );
    }

    #[test]
    fn unknown_keys_warn_and_do_not_fail() {
        let load = Config::from_toml("future = 1\npacks = [\"go\"]\n").unwrap();
        assert_eq!(load.config.packs, ["go"]);
        assert_eq!(load.warnings, ["unknown config key ignored: future"]);
    }

    #[test]
    fn merge_overlay_wins_only_for_set_keys() {
        let base = Config {
            packs: vec!["rust".into()],
            scripts: vec!["a.rhai".into()],
            prefix: Some("/old".into()),
            t2: T2Table {
                picks: BTreeMap::from([("java".into(), T2Backend::Heuristic)]),
            },
        };
        let overlay = ConfigOverlay {
            packs: Some(vec!["python".into()]),
            scripts: None,
            prefix: None,
            t2: None,
            warnings: vec![],
        };
        let merged = base.merge(&overlay);
        assert_eq!(merged.packs, ["python"]);
        assert_eq!(merged.scripts, ["a.rhai"]);
        assert_eq!(merged.prefix.as_deref(), Some("/old"));
        assert_eq!(merged.t2_for("java"), T2Backend::Heuristic);
    }

    #[test]
    fn merge_overlay_can_clear_packs_when_set_empty() {
        let base = Config {
            packs: vec!["rust".into()],
            scripts: vec![],
            prefix: None,
            t2: T2Table::default(),
        };
        let overlay = ConfigOverlay {
            packs: Some(vec![]),
            scripts: None,
            prefix: Some("/n".into()),
            t2: None,
            warnings: vec![],
        };
        let merged = base.merge(&overlay);
        assert!(merged.packs.is_empty());
        assert_eq!(merged.prefix.as_deref(), Some("/n"));
    }

    #[test]
    fn merge_overlay_t2_is_per_language() {
        let mut base = Config::empty();
        base.t2.picks.insert("java".into(), T2Backend::Heuristic);
        base.t2.picks.insert("php".into(), T2Backend::Heuristic);
        let overlay = ConfigOverlay {
            packs: None,
            scripts: None,
            prefix: None,
            t2: Some(T2Table {
                picks: BTreeMap::from([("java".into(), T2Backend::StackGraphs)]),
            }),
            warnings: vec![],
        };
        let merged = base.merge(&overlay);
        assert_eq!(merged.t2_for("java"), T2Backend::StackGraphs);
        assert_eq!(merged.t2_for("php"), T2Backend::Heuristic);
    }

    #[test]
    fn to_toml_round_trips_known_keys() {
        let cfg = Config {
            packs: vec!["python".into(), "rust".into()],
            scripts: vec!["deny.rhai".into(), "watch.rhai".into()],
            prefix: Some("/tmp/x".into()),
            t2: T2Table {
                picks: BTreeMap::from([("java".into(), T2Backend::Heuristic)]),
            },
        };
        let snap = cfg.to_toml();
        assert!(snap.contains("\"deny.rhai\", \"watch.rhai\""));
        assert!(snap.contains("java = \"heuristic\""));
        let load = Config::from_toml(&snap).unwrap();
        assert_eq!(load.config, cfg);
        assert_eq!(Config::empty().to_toml(), "");
        let quote = Config {
            packs: vec!["a\"b".into()],
            scripts: vec![],
            prefix: None,
            t2: T2Table::default(),
        };
        assert!(quote.to_toml().contains("\\\""));
    }

    #[test]
    fn invalid_toml_and_types() {
        assert!(matches!(
            ConfigOverlay::parse("[["),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            ConfigOverlay::parse("packs = 1"),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            ConfigOverlay::parse("packs = [1]"),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            ConfigOverlay::parse("prefix = 3"),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            ConfigOverlay::parse("scripts = \"x\""),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            ConfigOverlay::parse("t2 = 1"),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            ConfigOverlay::parse("[t2]\njava = 1\n"),
            Err(ConfigError::Toml(_))
        ));
    }
}
