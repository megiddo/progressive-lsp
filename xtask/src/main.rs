//! cargo xtask: musl, check-static, bench-alloc.

mod allocator;
mod check_static;
mod dist;
mod musl;

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
        "check-static" => check_static::run(&args[1..]),
        "bench-alloc" => allocator::run(&args[1..]),
        "dist" => dist::run(&args[1..]),
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
xtask check-static <ELF>...
xtask bench-alloc
xtask dist [--slim|--full|--pack slim|full|python,rust,...] --dest DIR
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
        print_help();
        assert!(workspace_root().join("Cargo.toml").is_file());
    }
}
