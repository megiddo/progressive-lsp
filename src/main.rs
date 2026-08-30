fn main() {
    let mem = progressive_lsp_core::MemoryLog::new();
    let log: std::sync::Arc<dyn progressive_lsp_core::LogPort> = std::sync::Arc::new(mem);
    let _ = progressive_lsp_log::LogCrateBridge::try_install(std::sync::Arc::clone(&log));
    let _ = progressive_lsp_log::TracingBridge::try_install(std::sync::Arc::clone(&log));
    if progressive_lsp::run_with_log(std::env::args_os(), log).is_err() {
        std::process::exit(1);
    }
}
