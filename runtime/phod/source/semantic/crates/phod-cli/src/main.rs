#![forbid(unsafe_code)]

use phod_check::{check, CheckInput, DecisionStatus};
use phod_ir::{normalize, Candidate};
use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("phod-semantic: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, Box<dyn Error>> {
    match args {
        [command, format, path] if command == "normalize" && format == "legacy" => {
            let candidate = phod_legacy::adapt_json(&fs::read(path)?)?;
            write_json(&candidate)?;
            Ok(ExitCode::SUCCESS)
        }
        [command, format, path] if command == "normalize" && format == "ir" => {
            let candidate: Candidate = serde_json::from_slice(&fs::read(path)?)?;
            write_json(&normalize(candidate)?)?;
            Ok(ExitCode::SUCCESS)
        }
        [command, path] if command == "check" => {
            let input: CheckInput = serde_json::from_slice(&fs::read(path)?)?;
            let decision = check(&input);
            let accepted = decision.status == DecisionStatus::Accepted;
            write_json(&decision)?;
            Ok(if accepted { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        _ => Err("usage: phod-semantic normalize <legacy|ir> <fixture.json> | phod-semantic check <fixture.json>".into()),
    }
}

fn write_json(value: &impl serde::Serialize) -> Result<(), Box<dyn Error>> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    writeln!(handle)?;
    Ok(())
}
