use qroc_standalone::{Error, Result, bridge::Bridge};
use std::{io, path::PathBuf};
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--root" {
        return Err(Error::Invalid(
            "Usage: qroc-bridge --root ABSOLUTE_PRIVATE_ROOT (NDJSON on stdin/stdout)".into(),
        ));
    }
    let mut bridge = Bridge::open(&PathBuf::from(&args[1]))?;
    bridge.serve(io::stdin().lock(), io::stdout().lock())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
