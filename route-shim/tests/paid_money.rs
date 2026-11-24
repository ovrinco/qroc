//! Money guard on a fake provider (loopback port 0 only; no real provider, no key, no paid call).
//! Ceiling at the boundary, reservation released on settlement, a retry reserved anew, unknown usage
//! kept unknown and reserved, refused and timed-out requests, unknown pricing, both providers' bodies,
//! streamed replies: a long one assembled whole and settled exactly, a cut one kept unknown and reserved.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const KEY: &str = "sk-fixtureSENTINEL-paid-0123456789";

#[derive(Clone)]
struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<(String, String, Value)>>>,
    mode: Arc<Mutex<String>>,
}

fn read_request(s: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut b = [0; 8192];
    loop {
        let n = s.read(&mut b).ok()?;
        if n == 0 {
            return None;
        }
        bytes.extend_from_slice(&b[..n]);
        if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..p]).into_owned();
            let len: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().to_owned())
                })
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if head.to_ascii_lowercase().contains("expect: 100-continue") && bytes.len() == p + 4 {
                s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").ok()?;
            }
            while bytes.len() < p + 4 + len {
                let n = s.read(&mut b).ok()?;
                if n == 0 {
                    return None;
                }
                bytes.extend_from_slice(&b[..n]);
            }
            let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
            return Some((path, head, bytes[p + 4..p + 4 + len].to_vec()));
        }
    }
}

fn fake() -> Fake {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let f = Fake {
        port: l.local_addr().unwrap().port(),
        seen: Arc::default(),
        mode: Arc::new(Mutex::new("ok".into())),
    };
    let g = f.clone();
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let g = g.clone();
            thread::spawn(move || {
                let Some((path, head, body)) = read_request(&mut s) else {
                    return;
                };
                let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                g.seen
                    .lock()
                    .unwrap()
                    .push((path.clone(), head, body.clone()));
                let mode = g.mode.lock().unwrap().clone();
                let anthropic = path == "/v1/messages";
                let (status, reply) = match mode.as_str() {
                    "no-usage" if anthropic => (
                        200,
                        json!({"id":"msg_nousage","model":body["model"],"content":[{"type":"text","text":"answer"}],"stop_reason":"end_turn"}),
                    ),
                    "no-usage" => (
                        200,
                        json!({"id":"chatcmpl_nousage","model":body["model"],"choices":[{"message":{"content":"answer"},"finish_reason":"stop"}]}),
                    ),
                    "reject" => (400, json!({"error":{"message":"invalid request"}})),
                    "overloaded" => (529, json!({"error":{"type":"overloaded_error"}})),
                    "slow" => {
                        thread::sleep(Duration::from_millis(3000));
                        (200, json!({}))
                    }
                    "long" if anthropic => (
                        200,
                        json!({"id":"msg_long","model":body["model"],"content":[{"type":"text","text":long_text()}],"stop_reason":"end_turn","usage":{"input_tokens":500,"output_tokens":90,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"service_tier":"standard"}}),
                    ),
                    "long" => (
                        200,
                        json!({"id":"chatcmpl_long","model":body["model"],"service_tier":"default","choices":[{"message":{"content":long_text()},"finish_reason":"stop"}],"usage":{"prompt_tokens":500,"completion_tokens":90,"prompt_tokens_details":{"cached_tokens":0}}}),
                    ),
                    _ if anthropic => (
                        200,
                        json!({"id":"msg_fixture","model":body["model"],"content":[{"type":"thinking","thinking":""},{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"service_tier":"standard"}}),
                    ),
                    _ => (
                        200,
                        json!({"id":"chatcmpl_fixture","model":format!("{}-2026-10-01",body["model"].as_str().unwrap_or("")),"service_tier":"default","choices":[{"message":{"content":"answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5,"prompt_tokens_details":{"cached_tokens":4}}}),
                    ),
                };
                if status == 200 && body["stream"] == true {
                    stream_reply(&mut s, anthropic, &mode, &reply);
                    return;
                }
                let b = reply.to_string();
                let _ = s.write_all(format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len()).as_bytes());
            });
        }
    });
    f
}

/// 400 numbered pieces, sent one delta each.
fn long_text() -> String {
    (0..400).map(|i| format!("piece {i}; ")).collect()
}

/// The reply as the provider streams it (server-sent events, chunked). Modes: "long" spreads 400
/// deltas over about a second; "cut" drops the connection mid-way, before the final usage;
/// "cut-after-usage" drops it after the usage, before the end marker; "stream-error" sends an error
/// event mid-way; "no-usage" ends cleanly without any usage.
fn stream_reply(s: &mut TcpStream, anthropic: bool, mode: &str, reply: &Value) {
    let text = if anthropic {
        reply["content"][1]["text"]
            .as_str()
            .or(reply["content"][0]["text"].as_str())
    } else {
        reply["choices"][0]["message"]["content"].as_str()
    }
    .unwrap_or("")
    .to_owned();
    let pieces: Vec<String> = if mode == "long" {
        text.split_inclusive("; ").map(str::to_owned).collect()
    } else {
        vec![
            text[..text.len() / 2].to_owned(),
            text[text.len() / 2..].to_owned(),
        ]
    };
    let mut ev: Vec<String> = Vec::new();
    let mut tail: Vec<String> = Vec::new();
    let mark = |ev: &Vec<String>| ev.len();
    let (mut cut_at, mut usage_at) = (0, 0);
    if anthropic {
        let mut start = reply.clone();
        start["content"] = json!([]);
        start.as_object_mut().unwrap().remove("stop_reason");
        start["usage"]["output_tokens"] = json!(1);
        ev.push(format!(
            "event: message_start\ndata: {}\n\n",
            json!({"type":"message_start","message":start})
        ));
        ev.push("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".into());
        for (i, p) in pieces.iter().enumerate() {
            ev.push(format!("event: content_block_delta\ndata: {}\n\n", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":p}})));
            if i == 0 {
                cut_at = mark(&ev);
                if mode == "stream-error" {
                    ev.push("event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\"}}\n\n".into());
                    break;
                }
            }
        }
        if mode != "stream-error" {
            ev.push("event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".into());
            let mut d =
                json!({"type":"message_delta","delta":{"stop_reason":reply["stop_reason"]}});
            if mode != "no-usage" {
                d["usage"] = json!({"output_tokens":reply["usage"]["output_tokens"]});
            }
            ev.push(format!("event: message_delta\ndata: {d}\n\n"));
            usage_at = mark(&ev);
            tail.push("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into());
        }
    } else {
        let head = json!({"id":reply["id"],"object":"chat.completion.chunk","model":reply["model"],"service_tier":reply["service_tier"]});
        for (i, p) in pieces.iter().enumerate() {
            let mut c = head.clone();
            c["choices"] = json!([{"index":0,"delta":{"content":p},"finish_reason":null}]);
            ev.push(format!("data: {c}\n\n"));
            if i == 0 {
                cut_at = mark(&ev);
            }
        }
        let mut c = head.clone();
        c["choices"] =
            json!([{"index":0,"delta":{},"finish_reason":reply["choices"][0]["finish_reason"]}]);
        ev.push(format!("data: {c}\n\n"));
        if mode != "no-usage" {
            let mut u = head.clone();
            u["choices"] = json!([]);
            u["usage"] = reply["usage"].clone();
            ev.push(format!("data: {u}\n\n"));
        }
        usage_at = mark(&ev);
        tail.push("data: [DONE]\n\n".into());
    }
    let send = match mode {
        "cut" => cut_at,
        "cut-after-usage" => usage_at,
        _ => ev.len(),
    };
    ev.extend(tail);
    if s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").is_err() {
        return;
    }
    let gap = if mode == "long" {
        Duration::from_micros(1500)
    } else {
        Duration::ZERO
    };
    for e in &ev[..if mode.starts_with("cut") {
        send
    } else {
        ev.len()
    }] {
        if s.write_all(format!("{:x}\r\n{e}\r\n", e.len()).as_bytes())
            .is_err()
        {
            return;
        }
        let _ = s.flush();
        thread::sleep(gap);
    }
    if mode.starts_with("cut") {
        // The far side drops the connection mid-reply, with no closing chunk.
        let _ = s.shutdown(std::net::Shutdown::Both);
        return;
    }
    let _ = s.write_all(b"0\r\n\r\n");
}

struct Shim {
    child: Child,
    port: u16,
    root: PathBuf,
}
impl Drop for Shim {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn root(tag: &str) -> PathBuf {
    let base = std::env::var_os("ROUTE_SHIM_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let p = base.join(format!(
        "paid-money-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn prices() -> Value {
    json!({"schema":"qroc-prices/1","currency":"USD","models":{
        "anthropic:fake-claude":{"provider":"anthropic","model":"fake-claude","input":"4","cache_write_5m":"5","cache_write_1h":"8","cached_input":"0.20","output":"20","source_url":"fixture","read_on":"2026-10-30"},
        "openai:fake-gpt":{"provider":"openai","model":"fake-gpt","input":"2.00","cached_input":"0.10","output":"10.00","long_context_above_input_tokens":272000,"long_input":"4.00","long_cached_input":"0.20","long_output":"15.00","source_url":"fixture","read_on":"2026-10-30"}}})
}

/// The shim with its own throwaway shared layer (Stratum's API layer): the layer's price table is this
/// test's PRICES.json (linked), QROC's cap is wide so the batch ceiling is the stop under test, keys come
/// from the fixture environment, and the layer's ledger and accounts live in this test's folder.
fn shim(root: &Path) -> Command {
    use std::os::unix::fs::PermissionsExt;
    let config = root.join("layer-config");
    if !config.exists() {
        std::fs::create_dir_all(&config).unwrap();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(root.join("PRICES.json"), config.join("prices.json")).unwrap();
        std::fs::write(
            config.join("caps.json"),
            json!({"schema":"stratum-caps/1","currency":"USD","tools":{"qroc":{"total":"100"}}})
                .to_string(),
        )
        .unwrap();
    }
    let mut c = Command::new(env!("CARGO_BIN_EXE_route-shim"));
    c.env("STRATUM_API_CONFIG", &config)
        .env("STRATUM_API_HOME", root.join("layer-home"))
        .env("STRATUM_API_ALLOW_ENV_KEYS", "1")
        .env("OPENAI_API_KEY", KEY)
        .env("ANTHROPIC_API_KEY", KEY);
    c
}

/// The shared layer's ledger lines for this test.
fn layer_ledger(s: &Shim) -> Vec<Value> {
    std::fs::read_to_string(s.root.join("layer-home/ledger.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn start(f: &Fake, tag: &str, ceiling: &str) -> Shim {
    let root = root(tag);
    let base = format!("http://127.0.0.1:{}", f.port);
    let routes = json!({"routes":{
        "anthropic:fake-claude":{"kind":"anthropic","base":base,"model":"fake-claude","key_env":"FIXTURE_ANTHROPIC_KEY","priced":true,"metered":true,"budget":50,"max_output_tokens":100,"effort":"medium"},
        "apikey:fake-gpt":{"kind":"apikey","provider":"openai","base":base,"model":"fake-gpt","key_env":"FIXTURE_OPENAI_KEY","priced":true,"metered":true,"budget":50,"max_output_tokens":100,"effort":"medium"}}});
    std::fs::write(root.join("routes.json"), routes.to_string()).unwrap();
    std::fs::write(root.join("PRICES.json"), prices().to_string()).unwrap();
    std::fs::write(
        root.join("CEILING.json"),
        json!({"ceiling":{"amount":ceiling,"currency":"USD"}}).to_string(),
    )
    .unwrap();
    let mut child = shim(&root)
        .args([
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--timeout-ms",
            "1500",
            "--max-concurrent",
            "1",
        ])
        .arg("--routes")
        .arg(root.join("routes.json"))
        .arg("--state")
        .arg(root.join("state"))
        .arg("--prices")
        .arg(root.join("PRICES.json"))
        .arg("--ceiling")
        .arg(root.join("CEILING.json"))
        .env("FIXTURE_ANTHROPIC_KEY", KEY)
        .env("FIXTURE_OPENAI_KEY", KEY)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let mut out = child.stdout.take().unwrap();
    let mut b = [0; 1];
    while out.read(&mut b).unwrap() == 1 && b[0] != b'\n' {
        line.push(b[0] as char);
    }
    let port = line.rsplit(':').next().unwrap().trim().parse().unwrap();
    Shim { child, port, root }
}

fn ask(s: &Shim, label: &str, content: &str) -> (u16, Value, Value) {
    ask_for(s, label, content, 100)
}

fn ask_for(s: &Shim, label: &str, content: &str, max_tokens: u64) -> (u16, Value, Value) {
    let body = json!({"model":label,"messages":[{"role":"user","content":content}],"stream":false,"max_tokens":max_tokens}).to_string();
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    let reply = serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    thread::sleep(Duration::from_millis(50));
    let rows = std::fs::read_to_string(s.root.join("state/receipts.jsonl")).unwrap();
    (
        status,
        reply,
        serde_json::from_str(rows.lines().last().unwrap()).unwrap(),
    )
}

fn money(s: &Shim) -> Value {
    serde_json::from_slice(&std::fs::read(s.root.join("state/money.json")).unwrap()).unwrap()
}

fn hits(f: &Fake) -> usize {
    f.seen.lock().unwrap().len()
}

// One user message "x": bound 512 + 32 + 4 + 1 = 549 tokens at the dearest input rate (1h cache write,
// 8000 nano) plus 100 output tokens at 20000 nano: 6,392,000 nano = 0.006392 USD.
const ONE: &str = "0.006392";

fn no_key_anywhere(root: &Path) {
    let mut files = 0;
    for e in walk(root) {
        files += 1;
        let t = std::fs::read(&e).unwrap();
        assert!(
            !String::from_utf8_lossy(&t).contains("SENTINEL"),
            "key in {e:?}"
        );
    }
    println!("no key value in 0 of {files} state files");
}
fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(p).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            v.extend(walk(&p))
        } else {
            v.push(p)
        }
    }
    v
}

#[test]
fn ceiling_refused_at_the_boundary() {
    let f = fake();
    let below = start(&f, "below", "0.006391999");
    let (st, reply, r) = ask(&below, "anthropic:fake-claude", "x");
    assert_eq!(st, 429, "{reply}");
    assert_eq!(r["money_refusal"], "spending ceiling reached");
    assert_eq!(hits(&f), 0, "nothing reached the provider");
    let at = start(&f, "at", ONE);
    let (st, reply, r) = ask(&at, "anthropic:fake-claude", "x");
    assert_eq!(st, 200, "{reply}");
    assert_eq!(r["reservation"]["amount"], "0.006392000");
    assert_eq!(hits(&f), 1);
    let (st, _, r) = ask(&at, "anthropic:fake-claude", "x");
    assert_eq!(
        st, 429,
        "settled 0.00014 + next 0.006392 exceeds the ceiling"
    );
    assert_eq!(hits(&f), 1);
    assert_eq!(r["budget_left"], 49, "a refused request spends no call");
    println!(
        "ceiling: one nano below refused unsent; equal admitted; next over refused (provider hits 1 of 3 asks)"
    );
    no_key_anywhere(&at.root);
}

#[test]
fn the_reply_says_the_output_limit_the_route_honoured() {
    let f = fake();
    let s = start(&f, "honoured", "1");
    // The route caps output at 100; QROC asked for 500.
    let (st, reply, r) = ask_for(&s, "anthropic:fake-claude", "x", 500);
    assert_eq!(st, 200);
    assert_eq!(reply["honoured_max_tokens"], 100);
    assert_eq!(r["max_tokens"], 100, "the receipt shows the same limit");
    assert_eq!(f.seen.lock().unwrap()[0].2["max_tokens"], 100);
}

#[test]
fn reservation_released_on_settlement() {
    let f = fake();
    let s = start(&f, "settle", "1");
    let (st, reply, r) = ask(&s, "anthropic:fake-claude", "x");
    assert_eq!(st, 200);
    // 10 input x 4000 + 5 output x 20000 = 140,000 nano.
    assert_eq!(r["charge"]["amount"], "0.000140000");
    assert_eq!(reply["charge"]["amount"], "0.000140000");
    assert_eq!(r["generation_id"], "msg_fixture");
    assert_eq!(r["completion_reason"], "end_turn");
    assert_eq!(r["served_model"], "fake-claude");
    assert_eq!(r["settled_total"], "0.000140000");
    assert_eq!(r["reserved_total"], "0.000000000");
    let m = money(&s);
    assert_eq!(m["settled_nano"], 140000);
    assert!(m["reservations"].as_object().unwrap().is_empty());
    let sent = f.seen.lock().unwrap()[0].clone();
    assert_eq!(sent.0, "/v1/messages");
    assert!(
        sent.1.contains("anthropic-version: 2026-10-21")
            && sent.1.to_ascii_lowercase().contains("x-api-key:")
    );
    assert_eq!(sent.2["max_tokens"], 100);
    assert_eq!(sent.2["output_config"]["effort"], "medium");
    let (st, _, r) = ask(&s, "apikey:fake-gpt", "x");
    assert_eq!(st, 200);
    // 6 uncached x 2000 + 4 cached x 100 + 5 x 10000 = 62,400 nano; dated snapshot matches its price.
    assert_eq!(r["charge"]["amount"], "0.000062400");
    assert_eq!(r["served_model"], "fake-gpt-2026-10-01");
    let sent = f.seen.lock().unwrap()[1].clone();
    assert_eq!(sent.0, "/v1/chat/completions");
    assert_eq!(sent.2["max_completion_tokens"], 100);
    assert!(sent.2.get("max_tokens").is_none());
    assert_eq!(sent.2["reasoning_effort"], "medium");
    assert_eq!(money(&s)["settled_nano"], 202400);
    // The same calls in the shared layer's one ledger, under tool qroc: admit then settle, twice.
    let l = layer_ledger(&s);
    let order: Vec<&str> = l.iter().map(|x| x["decision"].as_str().unwrap()).collect();
    assert_eq!(order, ["admit", "settle", "admit", "settle"]);
    assert!(l.iter().all(|x| x["tool"] == "qroc"));
    assert_eq!(l[1]["charge"], "0.000140000");
    assert_eq!(l[3]["charge"], "0.000062400");
    println!(
        "settlement: 2 of 2 reservations released, settled 0.000202400 USD across both providers; layer ledger admit→settle x2 under qroc"
    );
    no_key_anywhere(&s.root);
}

#[test]
fn retry_reserved_anew() {
    let f = fake();
    let s = start(&f, "retry", "1");
    let (_, _, first) = ask(&s, "anthropic:fake-claude", "same brief");
    let (_, _, second) = ask(&s, "anthropic:fake-claude", "same brief");
    assert_eq!(first["attempt"], 1);
    assert!(first["retry_of"].is_null());
    assert_eq!(second["attempt"], 2);
    assert_eq!(second["retry_of"], first["id"]);
    let log = std::fs::read_to_string(s.root.join("state/money.log.jsonl")).unwrap();
    let admits = log
        .lines()
        .filter(|l| l.contains("\"decision\":\"admit\""))
        .count();
    assert_eq!(admits, 2, "each attempt has its own reservation");
    assert_eq!(money(&s)["settled_nano"], 280000);
    println!("retry: 2 of 2 attempts reserved and settled separately; second names the first");
}

#[test]
fn unknown_usage_is_never_zero() {
    let f = fake();
    let s = start(&f, "unknown", "1");
    *f.mode.lock().unwrap() = "no-usage".into();
    let (st, reply, r) = ask(&s, "anthropic:fake-claude", "x");
    assert_eq!(st, 200, "the content is delivered");
    assert_eq!(reply["charge"]["amount"], "unknown");
    assert_eq!(r["charge"]["amount"], "unknown");
    assert_eq!(
        r["reserved_total"], "0.006392000",
        "the reservation stays counted"
    );
    let m = money(&s);
    assert_eq!(m["settled_nano"], 0);
    let id = r["id"].as_str().unwrap();
    assert_eq!(m["reservations"][id]["state"], "unresolved");
    *f.mode.lock().unwrap() = "ok".into();
    let (st, _, r2) = ask(&s, "anthropic:fake-claude", "y");
    assert_eq!(st, 429);
    assert!(r2["money_refusal"].as_str().unwrap().contains("unresolved"));
    assert_eq!(hits(&f), 1, "nothing admitted while a charge is unknown");
    let out = shim(&s.root)
        .args(["reconcile", "--state"])
        .arg(s.root.join("state"))
        .args([
            "--id",
            id,
            "--charge",
            "0.000140",
            "--evidence",
            "fixture: provider usage endpoint row",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (st, _, _) = ask(&s, "anthropic:fake-claude", "y");
    assert_eq!(st, 200, "admission resumes after reconciliation");
    // Timed out: unknown, kept reserved. Refused 400: not billed. Server error 529: unknown.
    *f.mode.lock().unwrap() = "slow".into();
    let (st, _, r) = ask(&s, "apikey:fake-gpt", "z");
    assert!(st == 504 || st == 502, "{st}");
    assert_eq!(r["charge"]["amount"], "unknown");
    assert!(
        r["charge"]["basis"]
            .as_str()
            .unwrap()
            .starts_with("unknown")
    );
    let tid = r["id"].as_str().unwrap().to_owned();
    shim(&s.root)
        .args(["reconcile", "--state"])
        .arg(s.root.join("state"))
        .args([
            "--id",
            &tid,
            "--charge",
            "0",
            "--evidence",
            "fixture: no generation on the provider",
        ])
        .output()
        .unwrap();
    *f.mode.lock().unwrap() = "reject".into();
    let (st, _, r) = ask(&s, "apikey:fake-gpt", "z");
    assert_eq!(st, 502);
    assert_eq!(r["charge"]["amount"], "0.000000000");
    assert!(r["charge"]["basis"].as_str().unwrap().contains("HTTP 400"));
    *f.mode.lock().unwrap() = "overloaded".into();
    let (_, _, r) = ask(&s, "anthropic:fake-claude", "z");
    assert_eq!(r["charge"]["amount"], "unknown");
    let status = shim(&s.root)
        .args(["status", "--state"])
        .arg(s.root.join("state"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(text.contains("unresolved=1"), "{text}");
    println!(
        "unknown: missing usage, timeout and 529 kept reserved as unknown (3 of 3); 400 settled not billed; reconcile resumes"
    );
    no_key_anywhere(&s.root);
}

#[test]
fn unknown_pricing_stops_admission() {
    let f = fake();
    let s = start(&f, "pricing", "1");
    let mut p = prices();
    p["models"]
        .as_object_mut()
        .unwrap()
        .remove("openai:fake-gpt");
    std::fs::write(s.root.join("PRICES.json"), p.to_string()).unwrap();
    let (st, _, r) = ask(&s, "apikey:fake-gpt", "x");
    assert_eq!(st, 503);
    assert!(
        r["money_refusal"]
            .as_str()
            .unwrap()
            .starts_with("unknown pricing")
    );
    assert_eq!(hits(&f), 0);
    std::fs::write(s.root.join("CEILING.json"), "{}").unwrap();
    let (st, _, r) = ask(&s, "anthropic:fake-claude", "x");
    assert_eq!(st, 503);
    assert!(
        r["money_refusal"]
            .as_str()
            .unwrap()
            .starts_with("no ceiling")
    );
    std::fs::write(
        s.root.join("CEILING.json"),
        json!({"amount":"1","currency":"USD"}).to_string(),
    )
    .unwrap();
    // A table whose priced route has no price does not serve at all.
    let out = shim(&s.root)
        .args(["serve", "--listen", "127.0.0.1:0", "--routes"])
        .arg(s.root.join("routes.json"))
        .arg("--state")
        .arg(s.root.join("state2"))
        .arg("--prices")
        .arg(s.root.join("PRICES.json"))
        .arg("--ceiling")
        .arg(s.root.join("CEILING.json"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no price for priced route"));
    println!(
        "pricing: unpriced model and missing ceiling refused unsent (2 of 2); serve refuses an unpriced table"
    );
}

fn reconcile(s: &Shim, id: &str, charge: &str) {
    let out = shim(&s.root)
        .args(["reconcile", "--state"])
        .arg(s.root.join("state"))
        .args([
            "--id",
            id,
            "--charge",
            charge,
            "--evidence",
            "fixture: provider usage row",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn long_reply_streamed_whole() {
    let f = fake();
    let s = start(&f, "long", "1");
    *f.mode.lock().unwrap() = "long".into();
    let t0 = std::time::Instant::now();
    let (st, reply, r) = ask(&s, "anthropic:fake-claude", "x");
    assert_eq!(st, 200, "{reply}");
    assert_eq!(
        reply["choices"][0]["message"]["content"],
        long_text(),
        "400 deltas assembled in order"
    );
    // 500 input x 4000 + 90 output x 20000 = 3,800,000 nano.
    assert_eq!(r["charge"]["amount"], "0.003800000");
    assert_eq!(r["generation_id"], "msg_long");
    assert_eq!(r["completion_reason"], "end_turn");
    let (st, reply, r) = ask(&s, "apikey:fake-gpt", "x");
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["choices"][0]["message"]["content"], long_text());
    // 500 uncached x 2000 + 90 x 10000 = 1,900,000 nano.
    assert_eq!(r["charge"]["amount"], "0.001900000");
    assert_eq!(r["generation_id"], "chatcmpl_long");
    let m = money(&s);
    assert_eq!(m["settled_nano"], 5_700_000);
    assert!(
        m["reservations"].as_object().unwrap().is_empty(),
        "both released"
    );
    let seen = f.seen.lock().unwrap().clone();
    assert_eq!(seen[0].2["stream"], true);
    assert_eq!(seen[1].2["stream"], true);
    assert_eq!(seen[1].2["stream_options"]["include_usage"], true);
    println!(
        "long stream: 2 of 2 replies of 400 deltas whole ({} ms), settled 0.005700000 USD exactly, both released",
        t0.elapsed().as_millis()
    );
    no_key_anywhere(&s.root);
}

#[test]
fn stream_cut_midway_is_unknown_and_reserved() {
    let f = fake();
    let s = start(&f, "cut", "1");
    let mut cut = 0;
    // Reservations of one "x": Anthropic as ONE; OpenAI 549 x 2000 + 100 x 10000 = 2,098,000 nano.
    for (label, reserved) in [
        ("anthropic:fake-claude", ONE),
        ("apikey:fake-gpt", "0.002098"),
    ] {
        *f.mode.lock().unwrap() = "cut".into();
        let (st, reply, r) = ask(&s, label, "x");
        assert_eq!(st, 502, "{reply}");
        assert_eq!(r["charge"]["amount"], "unknown", "{r}");
        assert!(
            r["charge"]["basis"]
                .as_str()
                .unwrap()
                .contains("stream cut before its final usage"),
            "{r}"
        );
        assert_eq!(
            r["reserved_total"],
            format!("{reserved}000"),
            "the reservation stays counted"
        );
        let id = r["id"].as_str().unwrap().to_owned();
        let m = money(&s);
        assert_eq!(m["reservations"][&id]["state"], "unresolved");
        assert_eq!(
            m["settled_nano"],
            cut * 100_000,
            "the cut one is not settled, at zero or anything"
        );
        let kept = std::fs::read_dir(s.root.join("state/failed"))
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{id}.provider-"))
            })
            .count();
        assert_eq!(kept, 1, "the cut stream is kept for reconciliation");
        *f.mode.lock().unwrap() = "ok".into();
        let (st, _, r2) = ask(&s, label, "y");
        assert_eq!(st, 429);
        assert!(r2["money_refusal"].as_str().unwrap().contains("unresolved"));
        reconcile(&s, &id, "0.000100");
        cut += 1;
    }
    // Cut after the final usage (before the end marker): the reply is whole and the charge exact.
    *f.mode.lock().unwrap() = "cut-after-usage".into();
    let (st, reply, r) = ask(&s, "apikey:fake-gpt", "x");
    assert_eq!(st, 200, "{reply}");
    assert_eq!(reply["choices"][0]["message"]["content"], "answer");
    assert_eq!(r["charge"]["amount"], "0.000062400");
    // An error event mid-stream: unknown, reserved.
    *f.mode.lock().unwrap() = "stream-error".into();
    let (st, _, r) = ask(&s, "anthropic:fake-claude", "x");
    assert_eq!(st, 502);
    assert_eq!(r["charge"]["amount"], "unknown");
    assert!(
        r["charge"]["basis"]
            .as_str()
            .unwrap()
            .contains("provider stream error"),
        "{r}"
    );
    assert_eq!(
        money(&s)["reservations"][r["id"].as_str().unwrap()]["state"],
        "unresolved"
    );
    assert_eq!(
        money(&s)["settled_nano"],
        200_000 + 62_400,
        "two reconciled at 0.0001 each, one exact"
    );
    println!(
        "cut stream: {cut} of 2 providers kept unknown and reserved until reconciled; cut after usage settled exactly; error event kept unknown"
    );
    no_key_anywhere(&s.root);
}
