//! Merged engine capabilities. OR across ready children.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EngineCapabilities {
    pub definition: bool,
    pub references: bool,
    pub hover: bool,
    pub implementation: bool,
    pub type_definition: bool,
}

impl EngineCapabilities {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn types_full() -> Self {
        Self {
            definition: true,
            references: true,
            hover: true,
            implementation: true,
            type_definition: true,
        }
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            definition: self.definition || other.definition,
            references: self.references || other.references,
            hover: self.hover || other.hover,
            implementation: self.implementation || other.implementation,
            type_definition: self.type_definition || other.type_definition,
        }
    }

    pub fn any(self) -> bool {
        self.definition || self.references || self.hover || self.implementation || self.type_definition
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_is_or_and_empty_has_none() {
        let a = EngineCapabilities {
            definition: true,
            ..EngineCapabilities::empty()
        };
        let b = EngineCapabilities {
            hover: true,
            implementation: true,
            ..EngineCapabilities::empty()
        };
        let m = a.merge(b);
        assert!(m.definition);
        assert!(!m.references);
        assert!(m.hover);
        assert!(m.implementation);
        assert!(!m.type_definition);
        assert!(m.any());
        assert!(!EngineCapabilities::empty().any());
        assert!(EngineCapabilities {
            definition: true,
            ..EngineCapabilities::empty()
        }
        .any());
        assert!(EngineCapabilities {
            references: true,
            ..EngineCapabilities::empty()
        }
        .any());
        assert!(EngineCapabilities {
            hover: true,
            ..EngineCapabilities::empty()
        }
        .any());
        assert!(EngineCapabilities {
            implementation: true,
            ..EngineCapabilities::empty()
        }
        .any());
        assert!(EngineCapabilities {
            type_definition: true,
            ..EngineCapabilities::empty()
        }
        .any());
        let full = EngineCapabilities::types_full();
        assert!(full.definition && full.references && full.hover && full.implementation && full.type_definition);
        assert_eq!(full.merge(EngineCapabilities::empty()), full);
    }
}
