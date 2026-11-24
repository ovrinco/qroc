use qroc_standalone::{
    Error, Result,
    harden::{Hardening, ask_questions, judge_answers},
    qroc::QrocStore,
};
use serde_json::json;
use std::path::PathBuf;
const USAGE: &str = "Usage: qroc-harden --root ABSOLUTE_ROOT (new --title TITLE --idea FILE | start --project ID | answer --project ID --answers FILE | show --project ID)";
fn flag(args: &[String], name: &str) -> Result<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
        .ok_or_else(|| Error::Invalid(USAGE.into()))
}
fn read(path: &str) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| Error::Invalid(format!("{path}: {e}")))
}
fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 || args[0] != "--root" {
        return Err(Error::Invalid(USAGE.into()));
    }
    let root = PathBuf::from(&args[1]);
    if !root.is_absolute() {
        return Err(Error::Invalid("Q-ROC root must be absolute".into()));
    }
    let rest = &args[3..];
    let ask = |idea: &str, s: &qroc_standalone::harden::Score| ask_questions(&root, idea, s);
    let out = match args[2].as_str() {
        "new" => QrocStore::open(&root)?.command(
            "qroc-project-create",
            &json!({"title": flag(rest, "--title")?, "brain_dump": read(&flag(rest, "--idea")?)?}),
        )?,
        "start" => {
            serde_json::to_value(Hardening::open(&root)?.start(&flag(rest, "--project")?, &ask)?)?
        }
        "answer" => serde_json::to_value(Hardening::open(&root)?.answer(
            &flag(rest, "--project")?,
            &read(&flag(rest, "--answers")?)?,
            &ask,
        )?)?,
        "judge" => serde_json::to_value(judge_answers(
            &root,
            &read(&flag(rest, "--idea")?)?,
            &serde_json::from_str::<Vec<String>>(&read(&flag(rest, "--questions")?)?)?,
            &serde_json::from_str::<Vec<String>>(&read(&flag(rest, "--answers")?)?)?,
        )?)?,
        "show" => serde_json::to_value(Hardening::open(&root)?.load(&flag(rest, "--project")?)?)?,
        _ => return Err(Error::Invalid(USAGE.into())),
    };
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{}", json!({"error": e.to_string()}));
        std::process::exit(1);
    }
}
