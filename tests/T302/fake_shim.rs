#[path = "../common/hardening.rs"]
mod hardening;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};
struct Bridge {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
    seq: u32,
    root: std::path::PathBuf,
}
impl Bridge {
    fn new(
        name: &str,
        provider: Option<&str>,
        tokens: &str,
        batch: Option<&str>,
        endpoint: &str,
    ) -> Self {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(format!(
                "roots/fake-{}-{name}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let mut c = Command::new(env!("CARGO_BIN_EXE_qroc-bridge"));
        c.arg("--root")
            .arg(&root)
            .env("QROC_GATEWAY", endpoint)
            .env("QROC_MODEL", "fixture-route")
            .env("QROC_MAX_TOKENS", tokens)
            .env("QROC_TIMEOUT_SECONDS", "45")
            .env_remove("QROC_PROVIDER")
            .env_remove("QROC_BATCH_MAX")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if let Some(p) = provider {
            c.env("QROC_PROVIDER", p);
        }
        if let Some(b) = batch {
            c.env("QROC_BATCH_MAX", b);
        }
        let mut child = c.spawn().unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        Self {
            child,
            out,
            seq: 0,
            root,
        }
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
        let r: Value = serde_json::from_str(&s).unwrap();
        println!("{cmd}: {r}");
        r
    }
    fn ok(&mut self, cmd: &str, args: Value) -> Value {
        let r = self.ask(cmd, args);
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }
    fn setup(&mut self, provider: &str) -> Value {
        let p = self.ok(
            "qroc-project-create",
            json!({"title":"Fixture","brain_dump":"A private provider truth fixture"}),
        );
        hardening::pass_hardening(&self.root.join("qroc"));
        let b=self.ok("qroc-branch-create",json!({"project_id":p["id"],"name":"Fixture","provider":provider,"model":"fixture-route"}));
        json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"STRATEGY_BRIEF"})
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.child.stdin.take();
        let _ = self.child.wait();
    }
}
fn fake(reason: &str, route: bool) -> (String, thread::JoinHandle<Value>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = l.local_addr().unwrap().to_string();
    let reason = reason.to_owned();
    let h = thread::spawn(move || {
        l.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut s = loop {
            match l.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(std::time::Instant::now() < deadline, "fake not contacted");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        };
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut len = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.strip_prefix("Content-Length: ") {
                len = n.trim().parse().unwrap();
            }
        }
        let mut bytes = vec![0; len];
        r.read_exact(&mut bytes).unwrap();
        let request = serde_json::from_slice(&bytes).unwrap();
        let mut reply = json!({"model":"reported-frontier","choices":[{"message":{"content":"# Fixture\n\nBounded private draft."},"finish_reason":reason}]});
        if route {
            reply["route"] = json!("returned-route");
        }
        let body = reply.to_string();
        write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        request
    });
    (endpoint, h)
}
#[test]
fn provider_truth_and_length() {
    for (reason, route) in [("stop", true), ("length", false)] {
        let (ep, h) = fake(reason, route);
        let mut b = Bridge::new(reason, Some("route-shim"), "8192", None, &ep);
        let cap = b.ok("qroc-registry-provider-capabilities", json!({}));
        assert_eq!(cap["provider"], "route-shim");
        let a = b.setup("route-shim");
        let j = b.ok("qroc-generate-start", a.clone());
        let mut job = Value::Null;
        for _ in 0..200 {
            job = b.ok("qroc-job-status", json!({"id":j["id"]}));
            if !["running", "queued"].contains(&job["state"].as_str().unwrap()) {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            job["state"],
            if reason == "stop" {
                "succeeded"
            } else {
                "partial"
            }
        );
        let docs = b.ok("qroc-document-list", a.clone());
        let d = &docs["documents"][0];
        assert_eq!(d["generation_inputs"]["provider"], "route-shim");
        assert_eq!(
            d["generation_inputs"]["route"],
            if route {
                "returned-route"
            } else {
                "fixture-route"
            }
        );
        assert_eq!(
            d["generation_inputs"]["response_model"],
            "reported-frontier"
        );
        assert_eq!(h.join().unwrap()["max_tokens"], 8192);
        if reason == "length" {
            let mut gate_args = a;
            gate_args["revision"] = d["revision"].clone();
            let gate = b.ok("qroc-gate-evaluate", gate_args);
            assert_eq!(
                gate["status"], "incomplete",
                "Incomplete draft admitted: {gate}"
            );
            assert!(gate.to_string().contains("incomplete"));
        }
    }
}
#[test]
fn defaults_validation_and_batch_bound() {
    let mut b = Bridge::new("default", None, "2049", None, "127.0.0.1:1");
    assert_eq!(
        b.ok("qroc-registry-provider-capabilities", json!({}))["provider"],
        "local-gateway"
    );
    let a = b.setup("local-gateway");
    let r = b.ask("qroc-generate-start", a);
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("2048"));
    for batch in [None, Some("1"), Some("2")] {
        let fixture = if batch == Some("2") {
            Some(fake("stop", true))
        } else {
            None
        };
        let ep = fixture
            .as_ref()
            .map(|v| v.0.as_str())
            .unwrap_or("127.0.0.1:1");
        let mut b = Bridge::new("batch", Some("route-shim"), "8192", batch, ep);
        let mut a = b.setup("route-shim");
        a["document_types"] = json!(["STRATEGY_BRIEF", "PRD"]);
        let r = b.ask("qroc-batch-start", a);
        if batch == Some("2") {
            assert_eq!(r["ok"], true, "{r}");
            assert_eq!(r["result"]["jobs"].as_array().unwrap().len(), 2);
            fixture.unwrap().1.join().unwrap();
        } else {
            assert_eq!(r["ok"], false);
            assert!(
                r["error"]
                    .as_str()
                    .unwrap()
                    .contains("capability-unavailable")
            );
        }
    }
    for p in ["BAD", "", "a_b"] {
        let mut b = Bridge::new("invalid", Some(p), "1024", None, "127.0.0.1:1");
        assert_eq!(
            b.ok("qroc-registry-provider-capabilities", json!({}))["available"],
            false
        );
    }
}
