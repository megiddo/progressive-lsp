//! cargo xtask: build / run help, plus musl, check-static, bench-alloc, poc.

mod allocator;
mod artifact_store;
mod check_static;
mod cli;
mod dist;
mod freshness;
mod musl;
mod pack;
mod perf;
mod poc;
mod runtime_image;
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
    cli::run(&args)
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
        run(vec!["build".into(), "--help".into()]).unwrap();
        run(vec!["run".into(), "--help".into()]).unwrap();
        cli::print_help(cli::HelpTopic::Root);
        assert!(workspace_root().join("Cargo.toml").is_file());
    }
}
