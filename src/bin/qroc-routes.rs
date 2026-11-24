//! Thin CLI over the routes table. Never prints a key.
use qroc_standalone::{
    Error, Result,
    routes::{self, Kind, Route, RouteTable},
};
use std::path::PathBuf;
const USAGE: &str = "Usage: qroc-routes --root ABSOLUTE_ROOT (list | add NAME KIND ENDPOINT MODEL [KEY_REF] | select NAME | show-request NAME | brief [NAME] PROMPT)";
fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}
fn run() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 || a[0] != "--root" {
        return Err(Error::Invalid(USAGE.into()));
    }
    let root = PathBuf::from(&a[1]);
    let mut t = RouteTable::open(&root)?;
    let rest = &a[3..];
    match (a[2].as_str(), rest.len()) {
        ("list", 0) => println!("{}", serde_json::to_string_pretty(&t.to_json())?),
        ("add", 4 | 5) => {
            t.add(Route {
                name: rest[0].clone(),
                kind: Kind::parse(&rest[1])?,
                endpoint: rest[2].clone(),
                model: rest[3].clone(),
                key_ref: rest.get(4).cloned(),
            })?;
            println!("added {}", rest[0]);
        }
        ("select", 1) => {
            t.select(&rest[0])?;
            println!("selected {}", rest[0]);
        }
        ("show-request", 1) => {
            let r = t
                .get(&rest[0])
                .ok_or_else(|| Error::Invalid(format!("no route named '{}'", rest[0])))?;
            let msgs = serde_json::json!([{"role":"user","content":"(prompt)"}]);
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &routes::build_openrouter_request(r, &msgs, 1024, &env)?.masked()
                )?
            );
        }
        ("brief", 1 | 2) => {
            let r = if rest.len() == 2 {
                t.get(&rest[0])
            } else {
                t.selected()
            }
            .ok_or_else(|| Error::Invalid("no route selected".into()))?
            .clone();
            let b = routes::send_brief(
                &root,
                &r,
                "You are Q-ROC. Write a concise product brief.",
                rest.last().unwrap(),
                1024,
                300,
                &env,
            )?;
            println!(
                "route={} receipts={}\n{}\n---\n{}",
                r.name,
                b.receipts_path.display(),
                b.receipt,
                b.content
            );
        }
        _ => return Err(Error::Invalid(USAGE.into())),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
