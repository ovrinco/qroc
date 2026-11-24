//! The Responses path through the layer's own service, against fake providers on loopback: OpenAI
//! passed through and settled from its final usage; Anthropic translated, text and a two-turn tool loop;
//! a cut stream stays reserved and blocks the tool; a turn over the cap is refused unsent.
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use stratum_api::{Layer, ledger, serve};

const OPENAI_KEY: &str = "sk-fixtureSENTINEL-responses-openai-0123";
const ANTHROPIC_KEY: &str = "sk-ant-fixtureSENTINEL-responses-4567";
const OPENROUTER_KEY: &str = "sk-or-v1-fixtureSENTINEL-responses-89ab";

/// A fake provider: records each request (path, headers, body) and answers from a queue of replies
/// (status, bytes, whether to cut the connection before the end).
/// One request a fake received: path, head, body.
type Seen = Arc<Mutex<Vec<(String, String, Value)>>>;
/// Queued replies: status and bytes.
type Replies = Arc<Mutex<Vec<(u16, Vec<u8>)>>>;

#[derive(Clone)]
struct Fake {
    port: u16,
    seen: Seen,
    replies: Replies,
}

impl Fake {
    fn start() -> Fake {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let f = Fake {
            port: l.local_addr().unwrap().port(),
            seen: Default::default(),
            replies: Default::default(),
        };
        let g = f.clone();
        thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let Some((path, head, body)) = read(&mut s) else {
                    continue;
                };
                g.seen.lock().unwrap().push((
                    path,
                    head,
                    serde_json::from_slice(&body).unwrap_or(Value::Null),
                ));
                let reply = {
                    let mut r = g.replies.lock().unwrap();
                    if r.is_empty() {
                        None
                    } else {
                        Some(r.remove(0))
                    }
                };
                let (status, bytes) = reply.unwrap_or((500, b"{}".to_vec()));
                let kind = if bytes.starts_with(b"{") {
                    "application/json"
                } else {
                    "text/event-stream"
                };
                let _ = s.write_all(
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                );
                let _ = s.write_all(&bytes);
            }
        });
        f
    }
    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    fn hits(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
    fn reply(&self, status: u16, bytes: impl Into<Vec<u8>>) {
        self.replies.lock().unwrap().push((status, bytes.into()));
    }
    fn request(&self, i: usize) -> (String, String, Value) {
        self.seen.lock().unwrap()[i].clone()
    }
}

fn read(s: &mut TcpStream) -> Option<(String, String, Vec<u8>)> {
    let (mut bytes, mut b) = (Vec::new(), [0u8; 16384]);
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

/// A layer in a throwaway folder: prices, caps (zeno 3 total, 1 a day unless given), routes to the fakes.
struct Setup {
    dir: PathBuf,
    port: u16,
    openai: Fake,
    anthropic: Fake,
}

impl Setup {
    fn new(name: &str, zeno_cap: Value) -> Setup {
        Setup::with(name, zeno_cap, false)
    }
    /// As `new`; with `openrouter`, a third route `responses:openrouter` to the OpenAI-form fake.
    fn with(name: &str, zeno_cap: Value, openrouter: bool) -> Setup {
        let dir =
            std::env::temp_dir().join(format!("stratum-responses-{name}-{}", std::process::id()));
        let (home, config) = (dir.join("home"), dir.join("config"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        // The layer's own owner-only key store, with fixture keys.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o700)).unwrap();
        let keys = config.join("keys.env");
        std::fs::write(
            &keys,
            format!("OPENAI_API_KEY={OPENAI_KEY}\nANTHROPIC_API_KEY={ANTHROPIC_KEY}\nOPENROUTER_API_KEY={OPENROUTER_KEY}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&keys, std::fs::Permissions::from_mode(0o600)).unwrap();
        let (openai, anthropic) = (Fake::start(), Fake::start());
        std::fs::write(config.join("prices.json"), json!({"schema":"qroc-prices/1","currency":"USD","models":{
            "openai:gpt-6.1-sol":{"provider":"openai","model":"gpt-6.1-sol","input":"2.00","cached_input":"0.10","output":"10.00",
                "long_context_above_input_tokens":272000,"long_input":"4.00","long_cached_input":"0.20","long_output":"15.00","source_url":"fixture","read_on":"2026-10-30"},
            "openrouter:qwen/qwen3.8-27b":{"provider":"openrouter","model":"qwen/qwen3.8-27b","input":"0.30","cached_input":"0.03","output":"1.20","source_url":"fixture","read_on":"2026-10-30"},
            "anthropic:claude-opus-5-5":{"provider":"anthropic","model":"claude-opus-5-5","input":"4","cache_write_5m":"5","cache_write_1h":"8","cached_input":"0.20","output":"20","source_url":"fixture","read_on":"2026-10-30"}}}).to_string()).unwrap();
        std::fs::write(
            config.join("caps.json"),
            json!({"schema":"stratum-caps/1","currency":"USD","tools":{"zeno":zeno_cap}})
                .to_string(),
        )
        .unwrap();
        let mut routes = json!({"routes":{
            "responses:openai":{"provider":"openai","model":"gpt-6.1-sol","base":openai.base(),"max_output_tokens":4000,"effort":"medium"},
            "responses:anthropic":{"provider":"anthropic","model":"claude-opus-5-5","base":anthropic.base(),"max_output_tokens":4000,"effort":"medium"}}});
        if openrouter {
            routes["routes"]["responses:openrouter"] = json!({"provider":"openrouter","model":"qwen/qwen3.8-27b","base":openai.base(),"max_output_tokens":4000,"effort":"medium"});
        }
        std::fs::write(config.join("routes.json"), routes.to_string()).unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let layer = Layer::at(home, config);
        thread::spawn(move || serve::run(layer, l, Duration::from_secs(2), 20_000));
        Setup {
            dir,
            port,
            openai,
            anthropic,
        }
    }
    fn layer(&self) -> Layer {
        Layer::at(self.dir.join("home"), self.dir.join("config"))
    }
    /// POST a Responses request as Zeno does; the status and the whole body.
    fn post(&self, tool: &str, body: &Value) -> (u16, String) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        let b = body.to_string();
        s.write_all(format!("POST /tool/{tool}/v1/responses HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nContent-Length: {}\r\n\r\n{b}", b.len()).as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let out = String::from_utf8_lossy(&out).into_owned();
        let status = out
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let body = out
            .split_once("\r\n\r\n")
            .map(|x| x.1.to_owned())
            .unwrap_or_default();
        (status, body)
    }
    fn ledger(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("home").join("ledger.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
    fn reservations(&self) -> Value {
        ledger::read_accounts(&self.dir.join("home")).unwrap()["tools"]["zeno"]["reservations"]
            .clone()
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        // The test's own throwaway folder, made by this test in this run.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sse(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
        .collect()
}

/// The Responses events in a reply body (comments skipped).
fn events(body: &str) -> Vec<Value> {
    body.split("\n\n")
        .filter_map(|b| {
            b.lines()
                .find_map(|l| l.strip_prefix("data: "))
                .and_then(|d| serde_json::from_str(d).ok())
        })
        .collect()
}

fn zeno_turn(model: &str, input: Value) -> Value {
    json!({"model":model,"instructions":"You are Zeno.","input":input,
        "tools":[{"type":"function","name":"shell_command","description":"Run a command","parameters":{"type":"object","properties":{"command":{"type":"string"}}}}],
        "tool_choice":"auto","parallel_tool_calls":false,"reasoning":{"effort":"medium"},"store":false,"stream":true,
        "include":["reasoning.encrypted_content"],"prompt_cache_key":"thread-1","client_metadata":{"x":"y"}})
}

fn user(text: &str) -> Value {
    json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})
}

fn openai_stream(cut: bool) -> String {
    let mut ev = vec![
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_9","status":"in_progress","model":"gpt-6.1-sol-2026-10-01"}}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_9","role":"assistant","content":[]}}),
        json!({"type":"response.output_text.delta","sequence_number":2,"item_id":"msg_9","output_index":0,"content_index":0,"delta":"Done."}),
        json!({"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":{"type":"message","id":"msg_9","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done.","annotations":[]}]}}),
    ];
    if !cut {
        ev.push(json!({"type":"response.completed","sequence_number":4,"response":{"id":"resp_9","status":"completed","model":"gpt-6.1-sol-2026-10-01","service_tier":"default",
            "usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":200},"output_tokens":100,"output_tokens_details":{"reasoning_tokens":40},"total_tokens":1100}}}));
    }
    sse(&ev)
}

#[test]
fn openai_passed_through_and_settled_from_usage() {
    let t = Setup::new("openai", json!({"total":"3","per_day":"1"}));
    let stream = openai_stream(false);
    t.openai.reply(200, stream.clone());
    let (status, body) = t.post("zeno", &zeno_turn("gpt-6.1-sol", json!([user("hi")])));
    assert_eq!(status, 200);
    assert!(
        body.starts_with(&stream),
        "events reach Zeno unchanged, in order"
    );
    let (path, head, sent) = t.openai.request(0);
    assert_eq!(path, "/v1/responses");
    assert!(
        head.contains(&format!("Bearer {OPENAI_KEY}")),
        "the layer's key, not Zeno's"
    );
    assert_eq!(sent["model"], "gpt-6.1-sol");
    assert_eq!(
        sent["max_output_tokens"], 4000,
        "the route's cap, so the reservation bounds the charge"
    );
    assert_eq!(sent["reasoning"]["effort"], "medium");
    assert_eq!(sent["store"], false);
    assert!(sent.get("client_metadata").is_none());
    assert_eq!(sent["input"], json!([user("hi")]));
    let l = t.ledger();
    let admit = l
        .iter()
        .find(|x| x["decision"] == "admit")
        .expect("admitted before it was sent");
    let settle = l.iter().find(|x| x["decision"] == "settle").unwrap();
    assert_eq!(
        (admit["tool"].as_str(), admit["route"].as_str()),
        (Some("zeno"), Some("responses:openai"))
    );
    // 800 × 2.00 + 200 × 0.10 + 100 × 10.00 per million tokens.
    assert_eq!(settle["charge"], "0.002620000");
    assert_eq!(settle["served_model"], "gpt-6.1-sol-2026-10-01");
    assert!(body.contains(&format!(
        ": stratum-api receipt {} charge 0.002620000 USD",
        settle["id"].as_str().unwrap()
    )));
    assert_eq!(t.reservations(), json!({}));
}

#[test]
fn anthropic_text_turn_translated_and_settled() {
    let t = Setup::new("anthropic-text", json!({"total":"3","per_day":"1"}));
    t.anthropic.reply(
        200,
        sse(&responses_translate_turns::text_turn(
            "msg_1",
            "Hello there",
        )),
    );
    let (status, body) = t.post("zeno", &zeno_turn("claude-opus-5-5", json!([user("hi")])));
    assert_eq!(status, 200);
    let ev = events(&body);
    let kinds: Vec<&str> = ev.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            "response.created",
            "response.output_item.added",
            "response.output_text.delta",
            "response.output_text.delta",
            "response.output_item.done",
            "response.completed"
        ]
    );
    let (path, head, sent) = t.anthropic.request(0);
    assert_eq!(path, "/v1/messages");
    assert!(
        head.contains(&format!("x-api-key: {ANTHROPIC_KEY}"))
            && head.contains("anthropic-version: 2026-10-21")
    );
    assert_eq!(
        (sent["model"].as_str(), sent["max_tokens"].as_u64()),
        (Some("claude-opus-5-5"), Some(4000))
    );
    assert_eq!(sent["output_config"]["effort"], "medium");
    assert_eq!(sent["system"][0]["text"], "You are Zeno.");
    assert_eq!(
        sent["system"][0]["cache_control"],
        json!({"type":"ephemeral"}),
        "the fake sees the cache marks"
    );
    assert_eq!(
        sent["tools"][0]["cache_control"],
        json!({"type":"ephemeral"})
    );
    assert_eq!(
        sent["messages"][0]["content"][0]["cache_control"],
        json!({"type":"ephemeral"})
    );
    let settle = t
        .ledger()
        .into_iter()
        .find(|x| x["decision"] == "settle")
        .unwrap();
    // 1200 input × 4 (reserved at the dearest input rate, settled at the plain one) + 7 output × 20.
    assert_eq!(settle["charge"], "0.004940000");
    assert_eq!(settle["route"], "responses:anthropic");
}

#[test]
fn anthropic_two_turn_tool_loop_through_the_service() {
    let t = Setup::new("anthropic-tools", json!({"total":"3","per_day":"1"}));
    t.anthropic
        .reply(200, sse(&responses_translate_turns::tool_turn()));
    t.anthropic.reply(
        200,
        sse(&responses_translate_turns::text_turn(
            "msg_3",
            "It holds apples.",
        )),
    );
    let first = vec![user("What is in a.txt?")];
    let (_, body) = t.post("zeno", &zeno_turn("claude-opus-5-5", json!(first)));
    let items: Vec<Value> = events(&body)
        .into_iter()
        .filter(|e| e["type"] == "response.output_item.done")
        .map(|e| e["item"].clone())
        .collect();
    assert_eq!(
        items
            .iter()
            .map(|i| i["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["reasoning", "function_call"]
    );
    let mut second = first.clone();
    second.extend(items);
    second.push(json!({"type":"function_call_output","call_id":"toolu_1","output":"apples"}));
    let (status, body) = t.post("zeno", &zeno_turn("claude-opus-5-5", json!(second)));
    assert_eq!(status, 200);
    let last = events(&body).into_iter().last().unwrap();
    assert_eq!(last["type"], "response.completed");
    assert_eq!(
        last["response"]["output"][0]["content"][0]["text"],
        "It holds apples."
    );
    let (_, _, sent) = t.anthropic.request(1);
    assert_eq!(
        sent["messages"][1]["content"][0],
        json!({"type":"thinking","thinking":"Need the file.","signature":"SIG1"})
    );
    assert_eq!(sent["messages"][1]["content"][1]["type"], "tool_use");
    assert_eq!(
        sent["messages"][2]["content"][0],
        json!({"type":"tool_result","tool_use_id":"toolu_1","content":"apples","cache_control":{"type":"ephemeral"}})
    );
    let settles = t
        .ledger()
        .into_iter()
        .filter(|x| x["decision"] == "settle")
        .count();
    assert_eq!(settles, 2, "each turn one admitted, settled call");
}

#[test]
fn cut_stream_stays_reserved_and_blocks_the_tool() {
    let t = Setup::new("cut", json!({"total":"3","per_day":"1"}));
    t.openai.reply(200, openai_stream(true));
    let (status, body) = t.post("zeno", &zeno_turn("gpt-6.1-sol", json!([user("hi")])));
    assert_eq!(status, 200);
    let last = events(&body).into_iter().last().unwrap();
    assert_eq!(
        last["type"], "response.failed",
        "Zeno is told the turn did not finish"
    );
    let r = t.reservations();
    let (_, res) = r
        .as_object()
        .unwrap()
        .iter()
        .next()
        .expect("the reservation stays");
    assert_eq!(res["state"], "unresolved");
    // The next turn is refused before it is sent: the fake sees no second request.
    t.openai.reply(200, openai_stream(false));
    let (status, body) = t.post("zeno", &zeno_turn("gpt-6.1-sol", json!([user("again")])));
    assert_eq!(status, 429, "{body}");
    assert!(body.contains("refused this turn, unsent"));
    assert_eq!(t.openai.hits(), 1);
}

#[test]
fn over_the_cap_is_refused_unsent() {
    // A cap below one turn's reservation (4000 output tokens × 10.00 per million is 0.04 alone).
    let t = Setup::new("cap", json!({"total":"0.01"}));
    t.openai.reply(200, openai_stream(false));
    t.anthropic.reply(
        200,
        sse(&responses_translate_turns::text_turn("m", "Hello")),
    );
    for model in ["gpt-6.1-sol", "claude-opus-5-5"] {
        let (status, body) = t.post("zeno", &zeno_turn(model, json!([user("hi")])));
        assert_eq!(status, 429, "{body}");
        assert!(body.contains("\"code\":\"spend_cap\""));
    }
    assert_eq!(
        (t.openai.hits(), t.anthropic.hits()),
        (0, 0),
        "nothing reached a provider"
    );
    // A tool with no cap is refused too.
    let (status, _) = t.post("nocap", &zeno_turn("gpt-6.1-sol", json!([user("hi")])));
    assert_eq!(status, 503);
    assert_eq!(t.openai.hits(), 0);
    let _ = t.layer();
}

#[test]
fn provider_refusal_is_returned_and_not_billed() {
    let t = Setup::new("refused", json!({"total":"3","per_day":"1"}));
    t.anthropic.reply(401, format!("{{\"type\":\"error\",\"error\":{{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key {ANTHROPIC_KEY}\"}}}}"));
    let (status, body) = t.post("zeno", &zeno_turn("claude-opus-5-5", json!([user("hi")])));
    assert_eq!(status, 401);
    assert!(body.contains("authentication_error"));
    assert!(
        !body.contains(ANTHROPIC_KEY),
        "the key never reaches Zeno: {body}"
    );
    let settle = t
        .ledger()
        .into_iter()
        .find(|x| x["decision"] == "settle")
        .unwrap();
    assert_eq!(settle["charge"], "0.000000000");
    assert_eq!(
        t.reservations(),
        json!({}),
        "a refusal before generation does not block the tool"
    );
}

#[test]
fn openrouter_passed_through_with_its_own_key() {
    let t = Setup::with("openrouter", json!({"total":"3","per_day":"1"}), true);
    let stream = openai_stream(false).replace("gpt-6.1-sol-2026-10-01", "qwen/qwen3.8-27b");
    t.openai.reply(200, stream.clone());
    let (status, body) = t.post("zeno", &zeno_turn("qwen/qwen3.8-27b", json!([user("hi")])));
    assert_eq!(status, 200, "{body}");
    assert!(body.starts_with(&stream));
    let (path, head, sent) = t.openai.request(0);
    assert_eq!(path, "/v1/responses");
    assert!(
        head.contains(&format!("Bearer {OPENROUTER_KEY}")) && !head.contains(OPENAI_KEY),
        "OpenRouter's key, not OpenAI's"
    );
    assert_eq!(
        (sent["model"].as_str(), sent["max_output_tokens"].as_u64()),
        (Some("qwen/qwen3.8-27b"), Some(4000))
    );
    let settle = t
        .ledger()
        .into_iter()
        .find(|x| x["decision"] == "settle")
        .unwrap();
    assert_eq!(
        (settle["route"].as_str(), settle["provider"].as_str()),
        (Some("responses:openrouter"), Some("openrouter"))
    );
    // 800 × 0.30 + 200 × 0.03 + 100 × 1.20 per million tokens.
    assert_eq!(settle["charge"], "0.000366000");
    assert_eq!(t.reservations(), json!({}));
}

#[test]
fn unlisted_model_and_missing_key_refused_in_plain_words() {
    let t = Setup::with(
        "openrouter-unready",
        json!({"total":"3","per_day":"1"}),
        true,
    );
    let routes = t.dir.join("config/routes.json");
    let r = std::fs::read_to_string(&routes)
        .unwrap()
        .replace("\"qwen/qwen3.8-27b\"", "\"qwen/qwen3.8-27b:free\"");
    std::fs::write(&routes, r).unwrap();
    let keys = t.dir.join("config/keys.env");
    std::fs::write(
        &keys,
        format!("OPENAI_API_KEY={OPENAI_KEY}\nANTHROPIC_API_KEY={ANTHROPIC_KEY}\n"),
    )
    .unwrap();
    let (status, body) = t.post(
        "zeno",
        &zeno_turn("qwen/qwen3.8-27b:free", json!([user("hi")])),
    );
    assert_eq!(status, 503, "{body}");
    assert!(
        body.contains("the model qwen/qwen3.8-27b:free is not on OpenRouter's price list"),
        "{body}"
    );
    assert!(
        body.contains("no OpenRouter key is stored") && body.contains("nothing was sent"),
        "{body}"
    );
    assert_eq!(t.openai.hits(), 0);
    assert!(t.ledger().is_empty(), "nothing admitted, nothing reserved");
}

#[test]
fn cache_reads_and_writes_settled_at_their_rates() {
    let t = Setup::new("anthropic-cache", json!({"total":"3","per_day":"1"}));
    let mut turn = responses_translate_turns::text_turn("msg_c", "Hello there");
    turn[0]["message"]["usage"] = json!({"input_tokens":200,"cache_read_input_tokens":9000,"cache_creation_input_tokens":1000,"output_tokens":1,"service_tier":"standard"});
    t.anthropic.reply(200, sse(&turn));
    let (status, body) = t.post("zeno", &zeno_turn("claude-opus-5-5", json!([user("hi")])));
    assert_eq!(status, 200, "{body}");
    let done = events(&body).into_iter().last().unwrap();
    assert_eq!(
        done["response"]["usage"]["input_tokens"], 10200,
        "all input counted for Zeno"
    );
    assert_eq!(
        done["response"]["usage"]["input_tokens_details"]["cached_tokens"],
        9000
    );
    let settle = t
        .ledger()
        .into_iter()
        .find(|x| x["decision"] == "settle")
        .unwrap();
    // 200 × 4 + 9000 × 0.20 (cache read) + 1000 × 5 (5-minute cache write) + 7 × 20, per million tokens.
    assert_eq!(settle["charge"], "0.007740000");
}

#[test]
fn unknown_model_has_no_route() {
    let t = Setup::new("route", json!({"total":"3"}));
    let (status, body) = t.post("zeno", &zeno_turn("qwen", json!([user("hi")])));
    assert_eq!(status, 400);
    assert!(body.contains("gpt-6.1-sol") && body.contains("claude-opus-5-5"));
    assert_eq!(t.openai.hits() + t.anthropic.hits(), 0);
}

#[path = "responses_translate.rs"]
#[allow(dead_code)]
mod responses_translate_turns;
