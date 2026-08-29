fn main() {
    if let Err(e) = progressive_lsp::run(std::env::args_os()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
