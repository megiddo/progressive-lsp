//! `NullStderrAdapter` / `InheritStderrAdapter` — stdio policy Adapters.

/// Adapter. `stderr(Stdio::null())`. **Forbidden** on production pack spawn.
pub struct NullStderrAdapter;

impl NullStderrAdapter {
    pub fn forbidden_on_prod_spawn() -> bool {
        true
    }
}

/// Adapter. `stderr(Stdio::inherit())`. Operator / CI harness bins only.
pub struct InheritStderrAdapter;

impl InheritStderrAdapter {
    pub fn allowed_on_serve() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_stderr_adapter_forbidden_on_prod_pack_spawn() {
        assert!(NullStderrAdapter::forbidden_on_prod_spawn());
        assert!(!InheritStderrAdapter::allowed_on_serve());
    }
}
