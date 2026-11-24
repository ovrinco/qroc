use qroc_standalone::worktable::join;
use std::{env, path::Path, process::ExitCode};
const USAGE: &str = "usage: qroc-worktable-join QROC_ROOT PROJECT BRANCH RUN [--advice]";
fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    let (root, project, branch, run, advice) = match args.as_slice() {
        [r, p, b, run] => (r, p, b, run, false),
        [r, p, b, run, flag] if flag == "--advice" => (r, p, b, run, true),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match join(Path::new(root), project, branch, run, advice) {
        Ok(value) => {
            println!("{value:#}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("qroc-worktable-join: {error}");
            ExitCode::from(1)
        }
    }
}
