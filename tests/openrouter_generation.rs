//! An OpenRouter selection generates through T402's request builder and receipt, not the
//! loopback poster. The OpenRouter endpoint is an in-test loopback stand-in: no network.
#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::routes::{Kind, Route, RouteTable};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KEY_REF: &str = "QROC_T410_FIXTURE_KEY";
const SENTINEL: &str = "SENTINEL-T410-KEY";

fn root(name: &str) -> PathBuf {
    let p = std::path::PathBuf::from("/example/operator/.cache/qroc-gate-recovery/T04/test-roots")
        .join(format!(
            "t410-{}-{name}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(p.join("qroc")).unwrap();
    p
}

/// Stand-in for the OpenRouter endpoint: answers one request and hands back its headers and body.
fn stand_in() -> (String, mpsc::Receiver<(String, Value)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/api/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let (mut headers, mut length) = (String::new(), 0);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap();
            }
            headers.push_str(&line);
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes).unwrap();
        let body = json!({"model":"fixture/served","choices":[{"message":{"content":"# Strategy brief\n\nA brief from the stand-in."},"finish_reason":"stop"}]}).to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let _ = tx.send((headers, serde_json::from_slice(&bytes).unwrap()));
    });
    (url, rx)
}

struct Bridge {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
    seq: u32,
}
impl Bridge {
    fn new(root: &std::path::Path, key: Option<&str>) -> Self {
        let mut c = Command::new(env!("CARGO_BIN_EXE_qroc-bridge"));
        c.arg("--root")
            .arg(root)
            .env("QROC_MAX_TOKENS", "4096")
            .env("QROC_TIMEOUT_SECONDS", "45")
            .env_remove("QROC_GATEWAY")
            .env_remove("QROC_MODEL")
            .env_remove("QROC_PROVIDER")
            .env_remove(KEY_REF)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if let Some(k) = key {
            c.env(KEY_REF, k);
        }
        let mut child = c.spawn().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        Self { child, out, seq: 0 }
    }
    fn ask(&mut self, cmd: &str, args: Value) -> Value {
        self.seq += 1;
        writeln!(
            self.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"version":1,"id":self.seq.to_string(),"command":cmd,"args":args})
        )
        .unwrap();
        let mut s = String::new();
        self.out.read_line(&mut s).unwrap();
        serde_json::from_str(&s).unwrap()
    }
    fn ok(&mut self, cmd: &str, args: Value) -> Value {
        let r = self.ask(cmd, args);
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }
    /// One project behind a passed gate, on a branch pinned to the OpenRouter route.
    fn setup(&mut self, root: &std::path::Path) -> Value {
        let p = self.ok(
            "qroc-project-create",
            json!({"title":"OpenRouter fixture","brain_dump":"A tool that makes slides."}),
        );
        hardening::pass_hardening(&root.join("qroc"));
        let b = self.ok(
            "qroc-branch-create",
            json!({"project_id":p["id"],"name":"Main","provider":"openrouter","model":"fixture/model"}),
        );
        json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"STRATEGY_BRIEF"})
    }
    fn finish(&mut self, job: &Value) -> Value {
        let mut state = Value::Null;
        for _ in 0..500 {
            state = self.ok("qroc-job-status", json!({"id":job["id"]}));
            if !["running", "queued"].contains(&state["state"].as_str().unwrap()) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        state
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.child.stdin.take();
        let _ = self.child.wait();
    }
}

fn select_openrouter(root: &std::path::Path, endpoint: &str) {
    let mut table = RouteTable::open_with(&root.join("qroc"), &|_| None).unwrap();
    table
        .add(Route {
            name: "openrouter".into(),
            kind: Kind::Openrouter,
            endpoint: endpoint.into(),
            model: "fixture/model".into(),
            key_ref: Some(KEY_REF.into()),
        })
        .unwrap();
    table.select("openrouter").unwrap();
}

fn receipts(root: &std::path::Path) -> Vec<Value> {
    fs::read_to_string(root.join("qroc/receipts.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn openrouter_selection_generates_one_document_with_a_receipt_and_the_key_masked() {
    let p = root("generate");
    let (url, request) = stand_in();
    select_openrouter(&p, &url);
    let mut b = Bridge::new(&p, Some(SENTINEL));
    let cap = b.ok("qroc-registry-provider-capabilities", json!({}));
    assert_eq!(cap["available"], true, "{cap}");
    assert_eq!(cap["provider"], "openrouter");
    let a = b.setup(&p);
    let job = b.ok("qroc-generate-start", a.clone());
    let job = b.finish(&job);
    assert_eq!(job["state"], "succeeded", "{job}");
    let (headers, body) = request.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        headers.contains(&format!("Authorization: Bearer {SENTINEL}")),
        "the key reaches the endpoint only"
    );
    assert_eq!(body["model"], "fixture/model");
    assert_eq!(body["max_tokens"], 4096);
    let docs = b.ok("qroc-document-list", a);
    let doc = &docs["documents"][0];
    assert_eq!(doc["document_type"], "STRATEGY_BRIEF", "{docs}");
    assert_eq!(doc["generation_inputs"]["provider"], "openrouter");
    assert_eq!(doc["generation_inputs"]["response_model"], "fixture/served");
    let rows = receipts(&p);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "openrouter");
    assert_eq!(rows[0]["status"], 200);
    assert_eq!(rows[0]["reported_model"], "fixture/served");
    assert!(rows[0]["content_sha256"].is_string());
    let mut shown = 0;
    for entry in walk(&p) {
        let bytes = fs::read(&entry).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(SENTINEL),
            "key written to {}",
            entry.display()
        );
        shown += 1;
    }
    assert!(shown > 3);
}

#[test]
fn openrouter_without_a_key_refuses_plainly_sends_nothing_and_records_no_request() {
    let p = root("nokey");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "http://{}/api/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    select_openrouter(&p, &url);
    let mut b = Bridge::new(&p, None);
    let cap = b.ok("qroc-registry-provider-capabilities", json!({}));
    assert_eq!(cap["available"], false, "{cap}");
    assert_eq!(cap["needs_key"], true);
    let reason = cap["reason"].as_str().unwrap();
    assert!(reason.contains("needs a key"), "{reason}");
    assert!(!reason.contains(KEY_REF), "no variable name in {reason}");
    let a = b.setup(&p);
    let started = b.ask("qroc-generate-start", a);
    let refusal = if started["ok"] == true {
        let job = b.finish(&started["result"]);
        assert_eq!(job["state"], "failed", "{job}");
        job.to_string()
    } else {
        started["error"].to_string()
    };
    assert!(refusal.contains("no key set"), "{refusal}");
    assert!(
        listener.accept().is_err(),
        "nothing reaches the endpoint without a key"
    );
    let rows = receipts(&p);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["status"], "no-request");
    assert_eq!(rows[0]["bytes_out"], 0);
    // Hardening asks the same route and gets the same plain refusal, never a made-up question.
    let q = qroc_standalone::harden::ask_questions(
        &p.join("qroc"),
        "An idea",
        &qroc_standalone::harden::score("An idea"),
    );
    assert_eq!(q.status, "unavailable");
    assert!(q.reason.unwrap_or_default().contains("no key set"));
    assert_eq!(receipts(&p).len(), 2);
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}
