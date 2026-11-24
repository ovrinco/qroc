use crate::codex::{Failure, ResultData, run};
use serde_json::Value;
use std::{net::TcpStream, process::Command};
fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
pub fn invoke(
    r: &Value,
    body: &Value,
    client: &TcpStream,
    timeout: u64,
) -> Result<ResultData, Failure> {
    let key = std::env::var(r["key_env"].as_str().unwrap()).map_err(|_| Failure {
        status: 502,
        exit: None,
    })?;
    if key.contains(['\r', '\n']) {
        return Err(Failure {
            status: 502,
            exit: None,
        });
    }
    let mut b = body.clone();
    b["model"] = r["model"].clone();
    let input = format!(
        "url = {}\nheader = {}\nheader = \"Content-Type: application/json\"\ndata = {}\nmax-time = {}\n",
        quote(&format!(
            "{}/v1/chat/completions",
            r["base"].as_str().unwrap().trim_end_matches('/')
        )),
        quote(&format!("Authorization: Bearer {key}")),
        quote(&b.to_string()),
        (timeout as f64 / 1000.0).max(0.001)
    );
    let mut cmd = Command::new("/usr/bin/curl");
    cmd.args(["-sS", "-K", "-", "--fail-with-body"]);
    let ran = run(cmd, input.into_bytes(), client, timeout);
    let exit = ran.result?;
    let out = ran.events;
    let value = serde_json::from_slice(&out).map_err(|_| Failure {
        status: 502,
        exit: Some(exit),
    })?;
    Ok(ResultData {
        value,
        exit: Some(exit),
        honoured: body.get("max_tokens").is_some(),
        hash: None,
    })
}
