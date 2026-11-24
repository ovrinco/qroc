use qroc_standalone::{contracts, qroc::QrocStore};
use std::{env, path::Path, process::ExitCode};
const USAGE: &str = "usage: qroc-contracts run HARDENING_JSON ROOT | tail QROC_ROOT PROJECT BRANCH | tail-state QROC_ROOT PROJECT BRANCH";
fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    let outcome = match args.as_slice() {
        [command, input, root] if command == "run" => {
            contracts::run(Path::new(input), Path::new(root)).map(|_| None)
        }
        [command, root, project, branch] if command == "tail" => {
            let root = Path::new(root);
            QrocStore::open(root)
                .and_then(|store| store.tail_input(project, branch))
                .and_then(|input| contracts::run_tail(root, &input))
                .map(Some)
        }
        [command, root, project, branch] if command == "tail-state" => {
            contracts::tail_state(Path::new(root), project, branch).map(Some)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(Some(value)) => match serde_json::to_string_pretty(&value) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("qroc-contracts: {error}");
                ExitCode::from(1)
            }
        },
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("qroc-contracts: {error}");
            ExitCode::from(1)
        }
    }
}
