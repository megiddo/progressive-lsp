//! `config.toml` schema stub. Empty config is valid; unknown keys warn.

use crate::error::ConfigError;

/// Written when [`crate::PrefixLayout::ensure_dirs`] creates a missing file.
pub const EMPTY_CONFIG_TOML: &str = "# progressive-lsp config stub (M0)\n# All keys optional. Empty file is valid.\n";

/// Merged config. Required keys: none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub packs: Vec<String>,
    pub scripts: Vec<String>,
    pub prefix: Option<String>,
}

impl Config {
    pub fn empty() -> Self {
        Self {
            packs: Vec::new(),
            scripts: Vec::new(),
            prefix: None,
        }
    }

    pub fn from_toml(src: &str) -> Result<ConfigLoad, ConfigError> {
        ConfigOverlay::parse(src).map(|overlay| ConfigLoad {
            config: Config::empty().merge(&overlay),
            warnings: overlay.warnings.clone(),
        })
    }

    /// Overlay wins for keys it **sets**. Unset keys fall through.
    pub fn merge(&self, overlay: &ConfigOverlay) -> Self {
        Self {
            packs: overlay.packs.clone().unwrap_or_else(|| self.packs.clone()),
            scripts: overlay.scripts.clone().unwrap_or_else(|| self.scripts.clone()),
            prefix: overlay
                .prefix
                .clone()
                .or_else(|| self.prefix.clone()),
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
                other => overlay
                    .warnings
                    .push(format!("unknown config key ignored: {other}")),
            }
        }
        Ok(overlay)
    }
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
        };
        let overlay = ConfigOverlay {
            packs: Some(vec!["python".into()]),
            scripts: None,
            prefix: None,
            warnings: vec![],
        };
        let merged = base.merge(&overlay);
        assert_eq!(merged.packs, ["python"]);
        assert_eq!(merged.scripts, ["a.rhai"]);
        assert_eq!(merged.prefix.as_deref(), Some("/old"));
    }

    #[test]
    fn merge_overlay_can_clear_packs_when_set_empty() {
        let base = Config {
            packs: vec!["rust".into()],
            scripts: vec![],
            prefix: None,
        };
        let overlay = ConfigOverlay {
            packs: Some(vec![]),
            scripts: None,
            prefix: Some("/n".into()),
            warnings: vec![],
        };
        let merged = base.merge(&overlay);
        assert!(merged.packs.is_empty());
        assert_eq!(merged.prefix.as_deref(), Some("/n"));
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
    }
}
