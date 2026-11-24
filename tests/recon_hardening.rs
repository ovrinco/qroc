//! Internal recon in hardening: our own disk is read through Lorengine before the person is
//! asked. Lorengine is a fixture script here and the route a loopback stand-in on port 0; no
//! model is called and nothing leaves the machine.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const KNOWN: &str = "/example/operator/06_memory/runs/example/OUTCOME.md";
const PRIVATE: &str = "/example/operator/.local/share/ovrin-private/held.md";

fn root(name: &str) -> PathBuf {
    let root =
        PathBuf::from(std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"))
            .join(format!(
                "recon-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
    fs::create_dir_all(&root).unwrap();
    root
}

/// A stand-in `lorengine` that logs its arguments and answers with one known record and one
/// from the private folder.
fn lorengine(dir: &Path) -> PathBuf {
    let reply = json!({"hits":[
        {"store":"decisions","kind":"decision","path":KNOWN,"line":12,"reference":"OD-2026-10-04-08",
         "standing":"stated","written_by":"him","date":"2026-10-04",
         "excerpt":"The first release runs on the local decision model first; one operator runs it.","relevance":0.9},
        {"store":"namespace","path":PRIVATE,"line":1,"excerpt":"PRIVATE SENTINEL","relevance":0.99}]});
    fs::write(dir.join("reply.json"), reply.to_string()).unwrap();
    let script = dir.join("lorengine");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\ncat '{reply}'\n",
            log = dir.join("calls.log").display(),
            reply = dir.join("reply.json").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// Answers each request with the next reply and keeps the prompts.
fn gateway(replies: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let mut prompts = Vec::new();
        for content in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
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
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let body = json!({"model":"test","choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            let messages = request["messages"].as_array().unwrap();
            prompts.push(
                messages.last().unwrap()["content"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        prompts
    });
    (endpoint, server)
}

struct Bridge {
    child: Child,
    out: BufReader<ChildStdout>,
    n: u64,
}

impl Bridge {
    fn start(root: &Path, endpoint: &str, lorengine: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qroc-bridge"))
            .args(["--root", root.to_str().unwrap()])
            .env("QROC_GATEWAY", endpoint)
            .env("QROC_MODEL", "test")
            .env("QROC_PROVIDER", "local-gateway")
            .env("QROC_MAX_TOKENS", "1024")
            .env("QROC_LORENGINE", lorengine)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        Self { child, out, n: 0 }
    }
    fn ask(&mut self, command: &str, args: Value) -> Value {
        self.n += 1;
        writeln!(
            self.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"version":1,"id":self.n.to_string(),"command":command,"args":args})
        )
        .unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        reply["result"].clone()
    }
    fn settled(&mut self, project: &Value) -> Value {
        for _ in 0..1000 {
            let state = self.ask("qroc-harden-get", json!({"project_id":project}));
            if state["running"] == false {
                assert_eq!(state["error"], Value::Null, "{state}");
                return state["record"].clone();
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("hardening round did not settle");
    }
    fn stop(mut self) {
        self.child.stdin.take();
        assert!(self.child.wait().unwrap().success());
    }
}

const WEAK: &str = "Zavent Sheets edits the tracker rows.";

#[test]
fn our_own_disk_is_read_first_and_what_it_answers_is_marked_with_its_source() {
    let root = root("found");
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    let lorengine = lorengine(&tools);
    let questions = format!(
        "[FOUND] Who runs it | {KNOWN}:12 | one operator runs it\n[FOUND] Which device | /example/operator/invented.md:4 | a phone\n[FOUND] What the plane is | {KNOWN}:12 | the record at this source, cited by the walk's stand-in\n- Which device and system will the first release run on?\n- What does a failed edit leave behind?"
    );
    let (endpoint, server) = gateway(vec![
        json!({"answers":[{"index":0,"score":30,"reason":"too thin"}]}).to_string(),
        questions,
        json!({"answers":[{"index":0,"score":95,"reason":"specific"},{"index":1,"score":95,"reason":"specific"},{"index":2,"score":95,"reason":"specific"}]}).to_string(),
    ]);
    let mut bridge = Bridge::start(&root, &endpoint, &lorengine);
    let project = bridge.ask(
        "qroc-project-create",
        json!({"title":"Sheets","brain_dump":WEAK}),
    )["id"]
        .clone();
    bridge.ask("qroc-harden-start", json!({"project_id":project}));
    let record = bridge.settled(&project);
    let q = &record["rounds"][0]["questions"];
    assert_eq!(q["status"], "asked", "{record}");
    assert_eq!(q["recon"]["status"], "searched");
    let findings = q["recon"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "the private record is never kept");
    assert_eq!(findings[0]["path"], KNOWN);
    // What the disk answers is kept with its source; a source the recon never gave is refused.
    let found = q["found"].as_array().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["source"], format!("{KNOWN}:12"));
    assert_eq!(found[0]["gap"], "Who runs it");
    let note = q["reason"].as_str().unwrap();
    assert!(
        note.contains("1 line(s) marked found on disk named no source"),
        "{note}"
    );
    // The walker's case: a real source, words the record does not hold. Refused, and asked.
    assert!(
        note.contains("1 line(s) marked found on disk said what the cited record does not hold and were not kept; their gaps are asked: What the plane is"),
        "{note}"
    );
    let items = q["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[2], "What the plane is?");
    assert_eq!(q["evidence"].as_array().unwrap().len(), 3);
    let answers = json!([
        "A Pixel 9 on Android 16.",
        "The row keeps its last saved value.",
        "The shared plane is the tracker store."
    ])
    .to_string();
    bridge.ask(
        "qroc-harden-answer",
        json!({"project_id":project,"answers":answers}),
    );
    let record = bridge.settled(&project);
    let round2 = &record["rounds"][1];
    assert_eq!(round2["verdict"], "pass", "{record}");
    let hardened = round2["idea_text"].as_str().unwrap();
    assert!(hardened.contains(&format!(
        "Who runs it: one operator runs it [found on disk: {KNOWN}:12]"
    )));
    assert!(hardened.contains("Records on our own disk for the question"));
    assert!(!hardened.contains("PRIVATE SENTINEL"));
    assert!(!hardened.contains("invented.md"));
    assert!(!hardened.contains("cited by the walk's stand-in"));
    bridge.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 3);
    let asking = &prompts[1];
    assert!(asking.contains("Found on our own disk before anyone is asked"));
    assert!(asking.contains(&format!("[{KNOWN}:12] (OD-2026-10-04-08, written by him)")));
    assert!(!asking.contains("PRIVATE SENTINEL"));
    // The judge of the answers reads the evidence with its sources.
    assert!(prompts[2].contains("found on disk"));
    let calls = fs::read_to_string(tools.join("calls.log")).unwrap();
    assert_eq!(
        calls.lines().count(),
        4,
        "the idea, then each of the three questions:\n{calls}"
    );
    for call in calls.lines() {
        assert!(call.starts_with("ask --json -n "), "{call}");
        assert!(
            call.contains("--store campaigns,decisions,namespace"),
            "{call}"
        );
        assert!(!call.contains("voice"));
    }
    assert!(calls.lines().next().unwrap().ends_with(WEAK));
}

#[test]
fn an_unavailable_lorengine_is_recorded_and_the_person_is_still_asked() {
    let root = root("unavailable");
    let (endpoint, server) = gateway(vec![
        json!({"answers":[{"index":0,"score":30,"reason":"too thin"}]}).to_string(),
        "- Which device will the first release run on?".into(),
    ]);
    let mut bridge = Bridge::start(&root, &endpoint, &root.join("no-such-lorengine"));
    let project = bridge.ask(
        "qroc-project-create",
        json!({"title":"Sheets","brain_dump":WEAK}),
    )["id"]
        .clone();
    bridge.ask("qroc-harden-start", json!({"project_id":project}));
    let record = bridge.settled(&project);
    let q = &record["rounds"][0]["questions"];
    assert_eq!(q["status"], "asked");
    assert_eq!(q["recon"]["status"], "unavailable");
    assert!(
        q["recon"]["reason"]
            .as_str()
            .unwrap()
            .contains("Lorengine could not be started")
    );
    assert!(q.get("found").is_none());
    bridge.stop();
    let prompts = server.join().unwrap();
    assert!(!prompts[1].contains("Found on our own disk"));
}

#[test]
fn a_fenced_judge_reply_is_read_and_an_unreadable_one_is_kept_as_evidence() {
    let root = root("judge-shapes");
    let tools = root.join("tools");
    fs::create_dir_all(&tools).unwrap();
    let lorengine = lorengine(&tools);
    let (endpoint, server) = gateway(vec![
        format!(
            "```json\n{}\n```",
            json!({"answers":[{"index":0,"score":30,"reason":"too thin"}]})
        ),
        "- Which device will the first release run on?".into(),
        "I cannot judge these answers.".into(),
    ]);
    let mut bridge = Bridge::start(&root, &endpoint, &lorengine);
    let project = bridge.ask(
        "qroc-project-create",
        json!({"title":"Sheets","brain_dump":WEAK}),
    )["id"]
        .clone();
    bridge.ask("qroc-harden-start", json!({"project_id":project}));
    let record = bridge.settled(&project);
    assert_eq!(record["rounds"][0]["judgment"]["status"], "judged");
    assert_eq!(record["rounds"][0]["score"]["total"], 30);
    bridge.ask(
        "qroc-harden-answer",
        json!({"project_id":project,"answers":json!(["A Pixel 9."]).to_string()}),
    );
    let mut state = Value::Null;
    for _ in 0..1000 {
        state = bridge.ask("qroc-harden-get", json!({"project_id":project}));
        if state["running"] == false {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let error = state["error"]
        .as_str()
        .expect("an unreadable judgment fails");
    assert!(
        error.contains("Judge reply holds no JSON object"),
        "{error}"
    );
    let kept = error
        .split("the reply is kept at ")
        .nth(1)
        .expect("kept path");
    assert_eq!(
        fs::read_to_string(kept).unwrap(),
        "I cannot judge these answers."
    );
    assert_eq!(
        state["record"]["rounds"].as_array().unwrap().len(),
        1,
        "no round from it"
    );
    bridge.stop();
    assert_eq!(server.join().unwrap().len(), 3);
}
