use qroc_standalone::worktable::{ExportOptions, export};
use std::{env, path::Path, process::ExitCode};
const USAGE: &str = "usage: qroc-worktable-export QROC_ROOT PROJECT BRANCH [ACCOUNT]";
fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    let (root, project, branch, account) = match args.as_slice() {
        [r, p, b] => (r, p, b, None),
        [r, p, b, a] => (r, p, b, Some(a.clone())),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut options = ExportOptions::default();
    if let Some(a) = account {
        options.account = a;
    }
    match export(Path::new(root), project, branch, &options) {
        Ok(value) => {
            println!("{value:#}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("qroc-worktable-export: {error}");
            ExitCode::from(1)
        }
    }
}
