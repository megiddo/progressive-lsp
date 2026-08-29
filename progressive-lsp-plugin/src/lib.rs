//! `PluginRegistry` + `LanguageFactory`. No `dlopen`. No global registry.

use std::collections::BTreeMap;

use progressive_lsp_core::{LanguageId, UnsupportedLanguage};
use progressive_lsp_resolve::ResolverChain;

/// Language ids that have a Factory slot in v1. M0 slots are empty.
pub const KNOWN_LANGUAGE_SLOTS: &[&str] = &[
    "c",
    "cpp",
    "csharp",
    "rust",
    "javascript",
    "typescript",
    "css",
    "html",
    "python",
    "php",
    "java",
    "go",
    "zig",
];

/// Abstract Factory for one language. Empty slot → [`UnsupportedLanguage`].
pub trait LanguageFactory: Send + Sync {
    fn language_id(&self) -> LanguageId;
    fn grammar_id(&self) -> &str;
    /// T1 required; T2/T3 optional. Default is an empty chain (empty slot).
    fn resolver_chain(&self) -> ResolverChain {
        ResolverChain::empty()
    }
}

/// Composition-time Factory / Registry. Lookup is deterministic.
#[derive(Default)]
pub struct PluginRegistry {
    factories: BTreeMap<String, Box<dyn LanguageFactory>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            factories: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, factory: Box<dyn LanguageFactory>) {
        let key = factory.language_id().as_str().to_string();
        self.factories.insert(key, factory);
    }

    pub fn get(&self, id: &LanguageId) -> Result<&dyn LanguageFactory, UnsupportedLanguage> {
        self.factories
            .get(id.as_str())
            .map(|f| f.as_ref())
            .ok_or_else(|| UnsupportedLanguage::new(id.clone()))
    }

    pub fn contains(&self, id: &LanguageId) -> bool {
        self.factories.contains_key(id.as_str())
    }

    pub fn registered_ids(&self) -> Vec<LanguageId> {
        self.factories
            .keys()
            .map(|k| LanguageId::new(k.as_str()))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
}

/// Link-time registration hook. M0 registers nothing — slots stay empty.
pub fn register_builtins(_registry: &mut PluginRegistry) {}

/// Every known v1 language as a [`LanguageId`].
pub fn known_language_ids() -> Vec<LanguageId> {
    KNOWN_LANGUAGE_SLOTS
        .iter()
        .copied()
        .map(LanguageId::new)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubFactory {
        id: LanguageId,
        grammar: &'static str,
    }

    impl LanguageFactory for StubFactory {
        fn language_id(&self) -> LanguageId {
            self.id.clone()
        }

        fn grammar_id(&self) -> &str {
            self.grammar
        }
    }

    #[test]
    fn empty_slots_return_unsupported_without_panic() {
        let mut registry = PluginRegistry::new();
        register_builtins(&mut registry);
        assert!(registry.is_empty());
        assert!(registry.registered_ids().is_empty());
        for id in known_language_ids() {
            assert!(!registry.contains(&id));
            let err = match registry.get(&id) {
                Err(e) => e,
                Ok(_) => panic!("empty slot must be unsupported"),
            };
            assert_eq!(err.language, id);
            assert_eq!(
                err.to_string(),
                format!("unsupported language: {}", id.as_str())
            );
        }
        assert_eq!(KNOWN_LANGUAGE_SLOTS.len(), 13);
        assert_eq!(KNOWN_LANGUAGE_SLOTS[0], "c");
        assert_eq!(KNOWN_LANGUAGE_SLOTS[12], "zig");
    }

    #[test]
    fn unknown_language_is_also_unsupported() {
        let registry = PluginRegistry::default();
        let id = LanguageId::new("brainfuck");
        let err = match registry.get(&id) {
            Err(e) => e,
            Ok(_) => panic!("unknown language must be unsupported"),
        };
        assert_eq!(err.language.as_str(), "brainfuck");
    }

    #[test]
    fn register_is_deterministic_last_wins() {
        let mut registry = PluginRegistry::new();
        registry.register(Box::new(StubFactory {
            id: LanguageId::new("java"),
            grammar: "first",
        }));
        registry.register(Box::new(StubFactory {
            id: LanguageId::new("java"),
            grammar: "second",
        }));
        let factory = registry.get(&LanguageId::new("java")).unwrap();
        assert_eq!(factory.grammar_id(), "second");
        assert_eq!(factory.language_id().as_str(), "java");
        assert!(factory.resolver_chain().is_empty());
        assert_eq!(registry.registered_ids(), vec![LanguageId::new("java")]);
        assert!(registry.contains(&LanguageId::new("java")));
        assert!(!registry.contains(&LanguageId::new("rust")));
        assert!(!registry.is_empty());
    }

    #[test]
    fn register_builtins_does_not_install_dlopen_or_globals() {
        let mut a = PluginRegistry::new();
        let mut b = PluginRegistry::new();
        register_builtins(&mut a);
        register_builtins(&mut b);
        assert!(a.is_empty());
        assert!(b.is_empty());
        a.register(Box::new(StubFactory {
            id: LanguageId::new("go"),
            grammar: "go",
        }));
        assert!(b.get(&LanguageId::new("go")).is_err());
    }
}
