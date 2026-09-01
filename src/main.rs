fn main() {
    if progressive_lsp::run(std::env::args_os()).is_err() {
        std::process::exit(1);
    }
}
