//! A fake provider on a free loopback port (OpenAI chat completions and Anthropic Messages), lifted
//! from route-shim's `tests/paid_money.rs`: it records every request it receives (so a test can prove
//! a refused call never reached it) and replies by mode, streamed when asked.
#![allow(dead_code)]
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

pub const KEY: &str = "sk-fixtureSENTINEL-layer-0123456789";
/// The eight bytes of a PNG signature, base64: what the fake returns as each image.
pub const PNG_B64: &str = "iVBORw0KGgo=";

#[derive(Clone)]
pub struct Fake {
    pub port: u16,
    pub seen: Arc<Mutex<Vec<(String, String, Value)>>>,
    pub mode: Arc<Mutex<String>>,
}

impl Fake {
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn hits(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
    pub fn set(&self, mode: &str) {
        *self.mode.lock().unwrap() = mode.into();
    }
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

pub fn fake() -> Fake {
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
                let image = path.starts_with("/v1/images/");
                let n = body["n"].as_u64().unwrap_or(1);
                let refs = body["images"].as_array().map_or(0, Vec::len) as u64;
                let image_reply = json!({"created":1_793_000_000u64,"data":(0..n).map(|_| json!({"b64_json":PNG_B64})).collect::<Vec<_>>(),"usage":{"input_tokens":20 + 100 * refs,"input_tokens_details":{"text_tokens":20,"image_tokens":100 * refs},"output_tokens":107 * n,"total_tokens":20 + 100 * refs + 107 * n}});
                let (status, reply) = match mode.as_str() {
                    "no-usage" if image => {
                        let mut r = image_reply.clone();
                        r.as_object_mut().unwrap().remove("usage");
                        (200, r)
                    }
                    "moderation" if image => (
                        400,
                        json!({"error":{"message":"Your request was rejected by the safety system.","code":"moderation_blocked"}}),
                    ),
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
                    _ if image => (200, image_reply),
                    _ if anthropic => (
                        200,
                        json!({"id":"msg_fixture","model":body["model"],"content":[{"type":"text","text":"answer"}],"stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"service_tier":"standard"}}),
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

/// The reply streamed as the provider sends it; "cut" drops the connection before the final usage.
fn stream_reply(s: &mut TcpStream, anthropic: bool, mode: &str, reply: &Value) {
    let mut ev: Vec<String> = Vec::new();
    let text = "answer";
    let cut_at;
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
        ev.push(format!("event: content_block_delta\ndata: {}\n\n", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}})));
        cut_at = ev.len();
        ev.push(
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n"
                .into(),
        );
        let mut d = json!({"type":"message_delta","delta":{"stop_reason":reply["stop_reason"]}});
        if mode != "no-usage" {
            d["usage"] = json!({"output_tokens":reply["usage"]["output_tokens"]});
        }
        ev.push(format!("event: message_delta\ndata: {d}\n\n"));
        ev.push("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".into());
    } else {
        let head = json!({"id":reply["id"],"object":"chat.completion.chunk","model":reply["model"],"service_tier":reply["service_tier"]});
        let mut c = head.clone();
        c["choices"] = json!([{"index":0,"delta":{"content":text},"finish_reason":null}]);
        ev.push(format!("data: {c}\n\n"));
        cut_at = ev.len();
        let mut c = head.clone();
        c["choices"] = json!([{"index":0,"delta":{},"finish_reason":"stop"}]);
        ev.push(format!("data: {c}\n\n"));
        if mode != "no-usage" {
            let mut u = head.clone();
            u["choices"] = json!([]);
            u["usage"] = reply["usage"].clone();
            ev.push(format!("data: {u}\n\n"));
        }
        ev.push("data: [DONE]\n\n".into());
    }
    if s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").is_err() {
        return;
    }
    let n = if mode == "cut" { cut_at } else { ev.len() };
    for e in &ev[..n] {
        if s.write_all(format!("{:x}\r\n{e}\r\n", e.len()).as_bytes())
            .is_err()
        {
            return;
        }
    }
    if mode == "cut" {
        let _ = s.shutdown(std::net::Shutdown::Both);
        return;
    }
    let _ = s.write_all(b"0\r\n\r\n");
}

/// A throwaway layer: config (prices, caps, routes, an owner-only key store) and home in a fresh folder.
pub struct Root {
    pub dir: PathBuf,
}

impl Root {
    pub fn config(&self) -> PathBuf {
        self.dir.join("config")
    }
    pub fn home(&self) -> PathBuf {
        self.dir.join("home")
    }
    pub fn layer(&self) -> stratum_api::Layer {
        stratum_api::Layer::at(self.home(), self.config())
    }
    pub fn caps(&self, tools: Value) {
        std::fs::write(
            self.config().join("caps.json"),
            json!({"schema":"stratum-caps/1","currency":"USD","tools":tools}).to_string(),
        )
        .unwrap();
    }
    pub fn ledger(&self) -> Vec<Value> {
        std::fs::read_to_string(self.home().join("ledger.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
    pub fn accounts(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.home().join("accounts.json")).unwrap()).unwrap()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        // The test clears the scratch it made itself, inside its own throwaway folder.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn prices() -> Value {
    json!({"schema":"qroc-prices/1","currency":"USD","models":{
        "anthropic:fake-claude":{"provider":"anthropic","model":"fake-claude","input":"4","cache_write_5m":"5","cache_write_1h":"8","cached_input":"0.20","output":"20","source_url":"fixture","read_on":"2026-10-30"},
        "openai:fake-gpt":{"provider":"openai","model":"fake-gpt","input":"2.00","cached_input":"0.10","output":"10.00","long_context_above_input_tokens":272000,"long_input":"4.00","long_cached_input":"0.20","long_output":"15.00","source_url":"fixture","read_on":"2026-10-30"}}})
}

pub fn root(tag: &str, f: &Fake) -> Root {
    let dir = std::env::temp_dir().join(format!(
        "stratum-api-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let r = Root { dir };
    std::fs::create_dir_all(r.config()).unwrap();
    std::fs::set_permissions(r.config(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(r.config().join("prices.json"), prices().to_string()).unwrap();
    r.caps(json!({"qroc":{"total":"1"},"zeno":{"total":"1","per_day":"0.5"}}));
    let base = f.base();
    std::fs::write(
        r.config().join("routes.json"),
        json!({"routes":{
            "openai:fake-gpt":{"provider":"openai","model":"fake-gpt","base":base,"max_output_tokens":100,"effort":"medium"},
            "anthropic:fake-claude":{"provider":"anthropic","model":"fake-claude","base":base,"max_output_tokens":100,"effort":"medium"}}})
        .to_string(),
    )
    .unwrap();
    let k = r.config().join("keys.env");
    std::fs::write(
        &k,
        format!("OPENAI_API_KEY={KEY}\nANTHROPIC_API_KEY={KEY}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&k, std::fs::Permissions::from_mode(0o600)).unwrap();
    r
}

/// No file under `dir` holds the fixture key value.
pub fn no_key_anywhere(dir: &Path) -> usize {
    let mut files = 0;
    for e in walk(dir) {
        if e.file_name().is_some_and(|n| n == "keys.env") {
            continue;
        }
        files += 1;
        let t = std::fs::read(&e).unwrap();
        assert!(
            !String::from_utf8_lossy(&t).contains("SENTINEL"),
            "key in {e:?}"
        );
    }
    files
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            v.extend(walk(&p))
        } else {
            v.push(p)
        }
    }
    v
}
