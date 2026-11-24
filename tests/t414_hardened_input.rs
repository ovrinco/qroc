#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::qroc::QrocStore;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn check(mode: &str, test: &str) {
    if private_gateway::parent_runs_child(test) {
        return;
    }
    let root = PathBuf::from(std::env::var_os("QROC_TEST_ROOT").expect("private root"))
        .join(format!("t414-{mode}-{}", std::process::id()));
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Hardening fixture","brain_dump":"Original weak idea"}),
        )
        .unwrap();
    hardening::pass_hardening(&root);
    let path = root
        .join("hardening")
        .join(format!("{}.json", project["id"].as_str().unwrap()));
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut passed = record["rounds"][0].clone();
    record["rounds"][0]["verdict"] = json!("refuse");
    record["rounds"][0]["sent_back"] = json!(true);
    record["rounds"][0]["answers"] = json!("HARDENED ANSWER SENTINEL");
    passed["round"] = json!(2);
    passed["idea_text"] = json!("Original weak idea\n\nHARDENED ANSWER SENTINEL");
    record["rounds"].as_array_mut().unwrap().push(passed);
    fs::write(&path, record.to_string()).unwrap();
    let branch = store.command("qroc-branch-create", &json!({"project_id":project["id"],"name":"Fixture","model":"t03-private-protocol-fixture","provider":"local-gateway"})).unwrap();
    let args = json!({"project_id":project["id"],"branch_id":branch["id"],"document_type":"STRATEGY_BRIEF","document_types":["STRATEGY_BRIEF"],"mode":mode});
    let result = store
        .command(
            if mode == "single" {
                "qroc-generate-start"
            } else {
                "qroc-batch-start"
            },
            &args,
        )
        .unwrap();
    let job = if mode == "single" {
        result
    } else {
        result["jobs"][0].clone()
    };
    let recorded: Value = serde_json::from_slice(
        &fs::read(
            root.join("jobs")
                .join(format!("{}.json", job["id"].as_str().unwrap())),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(recorded["hardening_round"], 2);
    assert_eq!(recorded["pinned_inputs"]["hardening_round"], 2);
    assert_eq!(
        recorded["pinned_inputs"]["project"]["brain_dump"],
        "Original weak idea\n\nHARDENED ANSWER SENTINEL"
    );
    private_gateway::completed(&mut store, &job);
}
#[test]
fn chain_batch_prompt_uses_passed_hardening_round() {
    check("chain", "chain_batch_prompt_uses_passed_hardening_round");
}
#[test]
fn graph_batch_prompt_uses_passed_hardening_round() {
    check("graph", "graph_batch_prompt_uses_passed_hardening_round");
}
#[test]
fn single_prompt_uses_passed_hardening_round() {
    check("single", "single_prompt_uses_passed_hardening_round");
}
mod private_gateway {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::{Command, Stdio},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };
    pub fn parent_runs_child(test_name: &str) -> bool {
        if std::env::var("QROC_PRIVATE_FIXTURE_CHILD").as_deref() == Ok(test_name) {
            return false;
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut count = 0;
            while !worker_stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(500)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_millis(500)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut buffer = [0u8; 8192];
                        let body_start = loop {
                            let n = stream.read(&mut buffer).unwrap();
                            assert!(n > 0);
                            request.extend_from_slice(&buffer[..n]);
                            assert!(request.len() < 4 * 1024 * 1024);
                            if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                                break i + 4;
                            }
                        };
                        let header = std::str::from_utf8(&request[..body_start]).unwrap();
                        assert!(header.starts_with("POST /v1/chat/completions HTTP/1.1"));
                        let length = header
                            .lines()
                            .find_map(|line| line.strip_prefix("Content-Length: "))
                            .unwrap()
                            .trim()
                            .parse::<usize>()
                            .unwrap();
                        assert!(length < 4 * 1024 * 1024);
                        while request.len() - body_start < length {
                            let n = stream.read(&mut buffer).unwrap();
                            assert!(n > 0);
                            request.extend_from_slice(&buffer[..n]);
                        }
                        let input: serde_json::Value =
                            serde_json::from_slice(&request[body_start..]).unwrap();
                        assert_eq!(input["model"], "t03-private-protocol-fixture");
                        assert_eq!(input["max_tokens"], 1024);
                        assert!(
                            input["messages"]
                                .to_string()
                                .contains("HARDENED ANSWER SENTINEL")
                        );
                        let body=serde_json::to_vec(&serde_json::json!({"choices":[{"message":{"content":"Private protocol fixture synthesis draft; no actual inference."},"finish_reason":"stop"}]})).unwrap();
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .unwrap();
                        stream.write_all(&body).unwrap();
                        count += 1;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => panic!("private fixture accept: {e}"),
                }
            }
            count
        });
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env("QROC_PRIVATE_FIXTURE_CHILD", test_name)
            .env("QROC_GATEWAY", address.to_string())
            .env("QROC_MODEL", "t03-private-protocol-fixture")
            .env("QROC_TIMEOUT_SECONDS", "15")
            .env("QROC_MAX_TOKENS", "1024")
            .env_remove("SOLITON_QROC_GATEWAY")
            .env_remove("SOLITON_QROC_TIMEOUT_SECONDS")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                break None;
            }
            thread::sleep(Duration::from_millis(5));
        };
        stop.store(true, Ordering::Relaxed);
        let requests = server.join().unwrap();
        assert!(
            status.is_some_and(|s| s.success()),
            "isolated child failed/timed out"
        );
        assert_eq!(
            requests, 1,
            "fixture must serve exactly one explicit request"
        );
        true
    }
    pub fn completed(
        store: &mut qroc_standalone::qroc::QrocStore,
        job: &serde_json::Value,
    ) -> serde_json::Value {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let status = store
                .command("qroc-job-status", &serde_json::json!({"id":job["id"]}))
                .unwrap();
            if status["state"] == "succeeded" {
                return status;
            }
            assert!(
                status["state"] == "running",
                "unexpected fixture state: {status}"
            );
            assert!(
                Instant::now() < deadline,
                "private fixture completion timed out"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}
