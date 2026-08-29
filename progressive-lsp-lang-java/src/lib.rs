//! Java LanguageFactory and T1 Tree-sitter intelligence. No JDK. No JVM.

#[cfg(test)]
mod bakeoff;
pub mod extract;
pub mod factory;
#[cfg(test)]
mod heuristic;
pub mod tokens;

#[cfg(test)]
mod f12;

pub use extract::JavaIndexer;
pub use factory::JavaLanguageFactory;
pub use tokens::{semantic_tokens_legend, SemanticToken, TOKEN_TYPES};

use progressive_lsp_core::LanguageId;

pub fn language_id() -> LanguageId {
    LanguageId::new("java")
}

pub fn grammar_id() -> &'static str {
    "tree-sitter-java"
}

pub fn tree_sitter_language() -> tree_sitter::Language {
    tree_sitter_java::LANGUAGE.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable() {
        assert_eq!(language_id().as_str(), "java");
        assert_eq!(grammar_id(), "tree-sitter-java");
        let _ = tree_sitter_language();
        assert!(!TOKEN_TYPES.is_empty());
        assert_eq!(semantic_tokens_legend().token_types.len(), TOKEN_TYPES.len());
    }
}
