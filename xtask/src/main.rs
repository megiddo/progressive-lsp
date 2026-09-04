//! cargo xtask: musl, check-static, bench-alloc, poc.

mod allocator;
mod check_static;
mod dist;
mod musl;
mod pack;
mod perf;
mod poc;
mod tarball;

use std::env;
use std::path::PathBuf;
use std::process;

fn main() {
    if let Err(e) = run(env::args().skip(1).collect()) {
        eprintln!("xtask: {e}");
        process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "musl" => musl::run(&args[1..]),
        "pack" => pack::run(&args[1..]),
        "check-static" => check_static::run(&args[1..]),
        "bench-alloc" => allocator::run(&args[1..]),
        "bench-perf" => perf::run(&args[1..]),
        "dist" => dist::run(&args[1..]),
        "poc" => poc::run(&args[1..]),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn print_help() {
    eprintln!(
        "\
xtask musl [--target TRIPLE] [--both]
  Core ELF only. Extracts to target/musl/<triple>/progressive-lsp then check-static.
xtask pack [--pack slim|python,rust,...] [--target TRIPLE] [--both]
  Slim packs only (ty, rust-analyzer, phpantom, biome, superhtml).
  Extracts to target/musl/<triple>/engines/<pack>/<binary> then check-static.
  Heavy packs (clangd/tsgo/gopls/zls) fail closed (HOST-7).
xtask check-static <ELF>...
xtask bench-alloc
xtask bench-perf
xtask dist [--slim|--full|--pack slim|full|python,rust,...] [--libc musl|glibc-static] --dest DIR
xtask poc [-- <poc-ide args>...]
"
    );
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a workspace member")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_and_unknown() {
        run(vec!["help".into()]).unwrap();
        assert!(run(vec!["nope".into()]).is_err());
        run(vec!["poc".into(), "--help".into()]).unwrap();
        print_help();
        assert!(workspace_root().join("Cargo.toml").is_file());
    }
}
