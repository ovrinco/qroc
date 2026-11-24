//! OpenRouter through the layer's service, on a stand-in (no network, no key of his): a listed model is
//! priced, settled and logged under the tool's cap; a model listed free is priced at zero explicitly; an
//! unlisted model is refused unsent as unknown pricing; with no OpenRouter key in the store the call is
//! refused unsent in plain words.
mod fake;
use fake::{fake, root};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpStream,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

fn post(port: u16, body: &Value) -> (u16, Value) {
    let b = body.to_string();
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.write_all(
        format!(
            "POST /tool/qroc/v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}",
            b.len()
        )
        .as_bytes(),
    )
    .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap(),
    )
}

#[test]
fn openrouter_priced_free_unlisted_and_keyless() {
    let f = fake();
    let r = root("openrouter", &f);
    let mut prices = fake::prices();
    prices["models"]["openrouter:fake/model"] = json!({"provider":"openrouter","model":"fake/model","input":"0.15","cached_input":"0.075","output":"0.60","source_url":"fixture","read_on":"2026-10-30"});
    prices["models"]["openrouter:fake/free:free"] = json!({"provider":"openrouter","model":"fake/free:free","input":"0","cached_input":"0","output":"0","source_url":"fixture","read_on":"2026-10-30"});
    std::fs::write(r.config().join("prices.json"), prices.to_string()).unwrap();
    std::fs::write(
        r.config().join("routes.json"),
        json!({"routes":{"openrouter:*":{"provider":"openrouter","base":f.base(),"max_output_tokens":100,"effort":"medium"}}}).to_string(),
    )
    .unwrap();
    let keys = r.config().join("keys.env");
    std::fs::write(&keys, format!("OPENROUTER_API_KEY={}\n", fake::KEY)).unwrap();
    std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_stratum-api"))
        .env("STRATUM_API_HOME", r.home())
        .env("STRATUM_API_CONFIG", r.config())
        .env_remove("STRATUM_API_ALLOW_ENV_KEYS")
        .args(["serve", "--listen", "127.0.0.1:0", "--idle-secs", "30"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let (mut line, mut b) = (String::new(), [0u8; 1]);
    while out.read(&mut b).unwrap() == 1 && b[0] != b'\n' {
        line.push(b[0] as char);
    }
    let port: u16 = line.rsplit(':').next().unwrap().trim().parse().unwrap();
    let ask = |label: &str| json!({"model":label,"messages":[{"role":"user","content":"x"}],"max_tokens":100});
    let (st, reply) = post(port, &ask("openrouter:fake/model"));
    assert_eq!(st, 200, "{reply}");
    // 6 uncached x 150 + 4 cached x 75 + 5 x 600 = 4,200 nano.
    assert_eq!(reply["charge"]["amount"], "0.000004200");
    let sent = f.seen.lock().unwrap()[0].clone();
    assert_eq!(sent.0, "/v1/chat/completions");
    assert!(sent.1.contains("Authorization: Bearer"));
    assert_eq!(sent.2["max_tokens"], 100);
    assert_eq!(sent.2["usage"]["include"], true);
    assert_eq!(sent.2["reasoning"]["effort"], "medium");
    let (st, reply) = post(port, &ask("openrouter:fake/free:free"));
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["charge"]["amount"], "0.000000000");
    assert!(
        reply["charge"]["basis"]
            .as_str()
            .unwrap()
            .contains("usage x openrouter:fake/free:free")
    );
    let (st, reply) = post(port, &ask("openrouter:other/unlisted"));
    assert_eq!(st, 503, "{reply}");
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .starts_with("unknown pricing"),
        "{reply}"
    );
    assert_eq!(f.hits(), 2, "the unlisted model never reached the provider");
    std::fs::write(&keys, "OPENAI_API_KEY=x-not-used-here\n").unwrap();
    let (st, reply) = post(port, &ask("openrouter:fake/model"));
    assert_eq!(st, 503, "{reply}");
    assert_eq!(
        reply["error"],
        "This route needs an OpenRouter key in the shared key store, and none is there"
    );
    assert_eq!(f.hits(), 2, "nothing is sent without a key");
    let _ = child.kill();
    let _ = child.wait();
    let ledger = r.ledger();
    let decisions: Vec<&str> = ledger
        .iter()
        .map(|l| l["decision"].as_str().unwrap())
        .collect();
    assert_eq!(decisions, ["admit", "settle", "admit", "settle", "refuse"]);
    assert!(
        ledger
            .iter()
            .all(|l| l["tool"] == "qroc" && l["provider"] == "openrouter")
    );
    let files = fake::no_key_anywhere(&r.dir);
    println!(
        "openrouter: listed model settled 0.0000042, free model at 0, unlisted refused unsent, keyless refused unsent in plain words (hits 2 of 4); no key in {files} files"
    );
}
