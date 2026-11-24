//! The local service and the command line, as installed: a tool's chat call through
//! `/tool/<tool>/v1/chat/completions` is admitted, sent to the (fake) provider, settled and readable in
//! `stratum-api spend`; a call over the cap is refused unsent; port 8080 and non-loopback addresses
//! are refused; the service exits by itself when idle.
mod fake;
use fake::{fake, root};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn post(port: u16, path: &str, body: &Value) -> (u16, Value) {
    let b = body.to_string();
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.write_all(format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len()).as_bytes()).unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap(),
    )
}

#[test]
fn service_admits_settles_refuses_and_exits_when_idle() {
    let f = fake();
    let r = root("service", &f);
    let cli = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_stratum-api"));
        c.env("STRATUM_API_HOME", r.home())
            .env("STRATUM_API_CONFIG", r.config())
            .env_remove("STRATUM_API_ALLOW_ENV_KEYS");
        c
    };
    let refused = cli()
        .args(["serve", "--listen", "127.0.0.1:8080"])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "port 8080 refused");
    let refused = cli()
        .args(["serve", "--listen", "0.0.0.0:0"])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "a non-loopback address refused");
    let mut child = cli()
        .args(["serve", "--listen", "127.0.0.1:0", "--idle-secs", "2"])
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
    let (st, reply) = post(
        port,
        "/tool/qroc/v1/chat/completions",
        &ask("anthropic:fake-claude"),
    );
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["choices"][0]["message"]["content"], "answer");
    assert_eq!(reply["charge"]["amount"], "0.000140000");
    assert_eq!(reply["generation_id"], "msg_fixture");
    let (st, reply) = post(
        port,
        "/tool/zeno/v1/chat/completions",
        &ask("openai:fake-gpt"),
    );
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["charge"]["amount"], "0.000062400");
    // qream has no cap: refused unsent.
    let (st, reply) = post(
        port,
        "/tool/qream/v1/chat/completions",
        &ask("openai:fake-gpt"),
    );
    assert_eq!(st, 503, "{reply}");
    assert_eq!(f.hits(), 2);
    // Over qroc's cap: refused unsent.
    r.caps(json!({"qroc":{"total":"0.000150"},"zeno":{"total":"1","per_day":"0.5"}}));
    let (st, reply) = post(
        port,
        "/tool/qroc/v1/chat/completions",
        &ask("anthropic:fake-claude"),
    );
    assert_eq!(st, 429, "{reply}");
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("tool cap reached")
    );
    assert_eq!(f.hits(), 2, "the refused call never reached the provider");
    let (st, _) = post(
        port,
        "/tool/Bad*Tool/v1/chat/completions",
        &ask("openai:fake-gpt"),
    );
    assert_eq!(st, 404);
    // Spend per tool and per day.
    let spend = cli().args(["spend", "--json"]).output().unwrap();
    let v: Value = serde_json::from_slice(&spend.stdout).unwrap();
    let q = v["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["tool"] == "qroc")
        .unwrap();
    assert_eq!(q["total"]["settled"], "0.000140000");
    assert_eq!(q["days"][0]["settled"], "0.000140000");
    assert_eq!(q["cap"], "0.000150000");
    let text = String::from_utf8(cli().arg("spend").output().unwrap().stdout).unwrap();
    assert!(
        text.contains("qroc: spent 0.00014 USD") && text.contains("zeno: spent 0.0000624 USD"),
        "{text}"
    );
    assert!(
        !text.contains("qream"),
        "a refused tool spends nothing and holds no account: {text}"
    );
    // Idle: no connection for two seconds and the service ends by itself.
    let t = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break Some(s);
        }
        if t.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        status.is_some_and(|s| s.success()),
        "the service exits when idle"
    );
    let files = fake::no_key_anywhere(&r.dir);
    println!(
        "service: 2 calls admitted and settled, uncapped and over-cap refused unsent (hits 2 of 4), spend read per tool and day, idle exit in {} ms, no key in {files} files",
        t.elapsed().as_millis()
    );
    println!("{text}");
}
