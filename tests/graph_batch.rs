#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{bridge::Bridge, qroc::QrocStore};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn root(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("/example/operator/.cache/qroc-gate-recovery/T04/test-roots").join(
        format!(
            "t403-{}-{}-{}",
            std::process::id(),
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ),
    )
}
#[test]
fn direct_and_bridge_export_agree_and_tail_does_not_generate() {
    let p = root("export");
    let mut store = QrocStore::open(&p.join("qroc")).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Test","brain_dump":"An idea"}),
        )
        .unwrap();
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project["id"],"name":"Test"}),
        )
        .unwrap();
    let args = json!({"project_id":project["id"],"branch_id":branch["id"]});
    let graph = store.graph_export(&args).unwrap();
    drop(store);
    let mut bridge = Bridge::open(&p).unwrap();
    let reply =
        bridge.reply(json!({"version":1,"id":"graph","command":"qroc-graph-export","args":args}));
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["result"], graph);
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 21);
    assert_eq!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["step"].is_string() && n["state"] == "waiting")
            .count(),
        4
    );
    assert_eq!(fs::read_dir(p.join("qroc/jobs")).unwrap().count(), 0);
}
#[test]
fn actual_batch_overlaps_siblings_bounds_width_and_pins_only_inputs() {
    exercise_batch("graph", 2);
    exercise_batch("chain", 1);
}
fn exercise_batch(mode: &'static str, expected_peak: usize) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let pair = Arc::new(Barrier::new(2));
    let (active_server, peak_server) = (active.clone(), peak.clone());
    let server = thread::spawn(move || {
        let mut threads = Vec::new();
        for index in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            let (active, peak, pair) = (active_server.clone(), peak_server.clone(), pair.clone());
            threads.push(thread::spawn(move || {
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader=BufReader::new(stream.try_clone().unwrap());
                let mut length=0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(n) = line.strip_prefix("Content-Length: ") {
                        length = n.trim().parse().unwrap();
                    }
                }
                let mut bytes=vec![0;length];reader.read_exact(&mut bytes).unwrap();
                let request: Value=serde_json::from_slice(&bytes).unwrap();
                let count=active.fetch_add(1,Ordering::SeqCst)+1;peak.fetch_max(count,Ordering::SeqCst);
                // The first two requests cannot finish unless both were launched.
                if mode == "graph" && index < 2 { pair.wait(); }
                thread::sleep(Duration::from_millis(60));
                let body=json!({"model":"test","choices":[{"message":{"content":"A private fixture draft."},"finish_reason":"stop"}]}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                active.fetch_sub(1,Ordering::SeqCst);
                request
            }));
        }
        threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    let runner = root("runner");
    let mut child = Command::new(env!("CARGO_BIN_EXE_qroc-bridge"))
        .args(["--root", runner.to_str().unwrap()])
        .env("QROC_GATEWAY", endpoint)
        .env("QROC_MODEL", "test")
        .env("QROC_PROVIDER", "local-gateway")
        .env("QROC_MAX_TOKENS", "1024")
        .env("QROC_TIMEOUT_SECONDS", "45")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let mut sequence = 0;
    let mut ask = |command: &str, args: Value| {
        sequence += 1;
        writeln!(
            child.stdin.as_mut().unwrap(),
            "{}",
            json!({"version":1,"id":sequence.to_string(),"command":command,"args":args})
        )
        .unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["result"].clone()
    };
    let project = ask(
        "qroc-project-create",
        json!({"title":"Fixture","brain_dump":"Private fixture idea"}),
    );
    hardening::pass_hardening(&runner.join("qroc"));
    let branch = ask(
        "qroc-branch-create",
        json!({"project_id":project["id"],"name":"Fixture","provider":"local-gateway","model":"test"}),
    );
    let owner = json!({"project_id":project["id"],"branch_id":branch["id"]});
    let mut saved = owner.clone();
    saved["document_type"] = json!("STRATEGY_BRIEF");
    saved["content"] = json!("BRIEF_ONLY");
    ask("qroc-document-save", saved);
    let mut args = owner.clone();
    args["document_types"] = json!([
        "PRE_MORTEM",
        "PRD",
        "VALIDATION_PLAN",
        "TECHNICAL_ARCHITECTURE"
    ]);
    args["width"] = json!(2);
    args["mode"] = json!(mode);
    let started = ask("qroc-batch-start", args);
    assert_eq!(started["width"], expected_peak);
    assert_eq!(started["execution"], mode);
    assert_eq!(
        started["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|j| j["state"] == "running")
            .count(),
        expected_peak
    );
    let mut graph = Value::Null;
    for _ in 0..200 {
        graph = ask("qroc-graph-export", owner.clone());
        if graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["state"] == "succeeded")
            .count()
            == 4
        {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["state"] == "succeeded")
            .count(),
        4
    );
    child.stdin.take();
    assert!(child.wait().unwrap().success());
    let requests = server.join().unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), expected_peak);
    for request in requests {
        let prompt = request["messages"][1]["content"].as_str().unwrap();
        if prompt.contains("Document: VALIDATION_PLAN")
            || prompt.contains("Document: TECHNICAL_ARCHITECTURE")
        {
            assert!(prompt.contains("Earlier branch document: PRD"));
            assert!(!prompt.contains("Earlier branch document: PRE_MORTEM"));
            assert!(!prompt.contains("Earlier branch document: STRATEGY_BRIEF"));
        }
    }
}
