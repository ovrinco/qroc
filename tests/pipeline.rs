//! Entire pipeline against the existing HTTP stand-in pattern; no inference.
use qroc_standalone::qroc::QrocStore;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn probe_binary(name: &str) -> PathBuf {
    std::env::var_os("QROC_PROBE_BIN_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_qroc-bridge")).with_file_name(name),
        |folder| PathBuf::from(folder).join(name),
    )
}
fn root(name: &str) -> PathBuf {
    let base =
        PathBuf::from(std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"))
            .join("pipeline-tests");
    base.join(format!(
        "{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn seed(root: &Path) -> String {
    let mut store = QrocStore::open(&root.join("qroc")).unwrap();
    let project = store.command("qroc-project-create", &json!({"title":"Metadata inspector", "brain_dump":"A rough idea: inspect scene metadata"})).unwrap();
    let id = project["id"].as_str().unwrap().to_owned();
    store.command("qroc-intake", &json!({"project_id":id,"text":"Build an offline pure scene metadata inspector. A scene has a nonempty title and an integer slides count of 1 through 100. Reject unknown fields. No writes or network effects. Return validity for the author before exporting."})).unwrap();
    id
}
fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let dest = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}
const ANSWERS: &str = r#"["Authors validate local metadata with nonempty title, slides in 1..100 inclusive, reject unknown fields, and a pure validity result; acceptance checks empty title, 0, 1, 100, 101 and extra fields, with no side effects."]"#;
struct Fixture {
    endpoint: String,
    stop: Arc<AtomicBool>,
    server: Option<thread::JoinHandle<()>>,
    calls: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    intervals: Arc<Mutex<Vec<(f64, f64)>>>,
    judges: Arc<AtomicUsize>,
    gates: Arc<AtomicUsize>,
    fail_gates: Arc<AtomicBool>,
}
impl Fixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let intervals = Arc::new(Mutex::new(Vec::new()));
        let (s, c, p, times) = (stop.clone(), calls.clone(), peak.clone(), intervals.clone());
        let judges = Arc::new(AtomicUsize::new(0));
        let gates = Arc::new(AtomicUsize::new(0));
        let fail_gates = Arc::new(AtomicBool::new(false));
        let (j, g, f) = (judges.clone(), gates.clone(), fail_gates.clone());
        let server = thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            let (judges, gates, fail_gates) = (j, g, f);
            let origin = Instant::now();
            let mut workers = Vec::new();
            while !s.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let (a, p, c, times, judges, gates) = (
                            active.clone(),
                            p.clone(),
                            c.clone(),
                            times.clone(),
                            judges.clone(),
                            gates.clone(),
                        );
                        let fail_gates = fail_gates.clone();
                        workers.push(thread::spawn(move || {
                            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut length = 0;
                            loop {
                                let mut line = String::new();
                                reader.read_line(&mut line).unwrap();
                                if line == "\r\n" { break; }
                                if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length: ") { length = n.trim().parse().unwrap(); }
                            }
                            let mut bytes = vec![0;length];
                            reader.read_exact(&mut bytes).unwrap();
                            let request: Value = serde_json::from_slice(&bytes).unwrap();
                            assert_eq!(request["model"], "test");
                            let prompt = request["messages"].as_array().unwrap().iter().filter_map(|m|m["content"].as_str()).collect::<Vec<_>>().join("\n");
                            c.fetch_add(1, Ordering::SeqCst);
                            p.fetch_max(a.fetch_add(1, Ordering::SeqCst)+1, Ordering::SeqCst);
                            let started = origin.elapsed().as_secs_f64();
                            thread::sleep(Duration::from_millis(120));
                            let content = if prompt.starts_with("Judge hardness") {
                                let score = if judges.fetch_add(1, Ordering::SeqCst)==0 { 60 } else { 95 };
                                json!({"answers":[{"index":0,"score":score,"reason":"Concrete boundary and measurable failure cases"}]}).to_string()
                            } else if prompt.contains("Ask the person who had the idea") {
                                "- What precise record boundaries and acceptance cases are required?".into()
                            } else if prompt.contains("Design Component contracts") {
                                json!([{"name":"Scene metadata", "purpose":"Inspect metadata", "inputs":["record"], "outputs":["validity"], "behaviour":["nonempty text: title","integer: slides in 1..100","unknown fields: reject"], "errors":["reject invalid record"],"effects":[],"profile":"qroc-record/1","record":{"text_field":"title","integer_field":"slides","minimum":1,"maximum":100,"unknown_fields":"reject"},"unsupported_reason":null}]).to_string()
                            } else if let Some(kind) = ["GATE_1","GATE_2","GATE_3"].into_iter().find(|k| prompt.contains(&format!("\nDocument: {k}\n"))) {
                                let rubric = qroc_standalone::gate_rubric::rubric(kind).unwrap().unwrap();
                                let low = kind=="GATE_1" && (gates.fetch_add(1, Ordering::SeqCst)==0 || fail_gates.load(Ordering::SeqCst));
                                let mut text = format!("[SCORE]: {}\n",if low {80}else{100});
                                for criterion in rubric.criteria {
                                    let score = if low {criterion.max * 8 / 10} else {criterion.max};
                                    text.push_str(&format!("- {}: {score}/{} - {}\n", criterion.name, criterion.max, if low {"Add the missing invalid input tests"}else{"All named cases covered"}));
                                }
                                text
                            } else { "Private stand-in document for the metadata inspector, with measured acceptance boundaries; no execution or approval claim.".into() };
                            let body = json!({"model":"test","choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string();
                            let _ = write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
                            times.lock().unwrap().push((started, origin.elapsed().as_secs_f64()));
                            a.fetch_sub(1, Ordering::SeqCst);
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            endpoint,
            stop,
            server: Some(server),
            calls,
            peak,
            intervals,
            judges,
            gates,
            fail_gates,
        }
    }
    fn reset(&self) {
        self.calls.store(0, Ordering::SeqCst);
        self.peak.store(0, Ordering::SeqCst);
        self.judges.store(0, Ordering::SeqCst);
        self.gates.store(0, Ordering::SeqCst);
        self.intervals.lock().unwrap().clear();
    }
    fn configure(&self, cmd: &mut Command) {
        cmd.env("QROC_GATEWAY", &self.endpoint)
            .env("QROC_MODEL", "test")
            .env("QROC_PROVIDER", "local-gateway")
            .env("QROC_TIMEOUT_SECONDS", "30")
            .env("QROC_MAX_TOKENS", "1024")
            .env(
                "QROC_PHOD_ADMITTED_BIN",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/runtime/phod/bin/phod-admitted"
                ),
            )
            .env(
                "QROC_PHOD_ADMITTED_SHA256",
                "ff169ec3d0deb230477e14985f93ba33f471cf7e4378c653cb1e3c064e8d9e52",
            )
            .env_remove("SOLITON_QROC_GATEWAY");
    }
    fn overlap(&self) -> f64 {
        let mut events = self
            .intervals
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(s, e)| [(*s, 1), (*e, -1)])
            .collect::<Vec<_>>();
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut active, mut last, mut overlap) = (0, 0.0, 0.0);
        for (at, delta) in events {
            if active >= 2 {
                overlap += at - last;
            }
            active += delta;
            last = at;
        }
        overlap
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.server.take().unwrap().join().unwrap();
    }
}
fn bridge_raw(fixture: &Fixture, root: &Path, args: Value, checker: Option<&str>) -> Value {
    let mut cmd = Command::new(probe_binary("qroc-bridge"));
    fixture.configure(&mut cmd);
    if let Some(checker) = checker {
        cmd.env("QROC_PHOD_ADMITTED_BIN", checker);
    }
    let mut child = cmd
        .args(["--root", root.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"version":1,"id":"pipeline","command":"qroc-run","args":args})
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let reply: Value = serde_json::from_str(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .nth(1)
            .unwrap(),
    )
    .unwrap();
    reply
}
fn bridge(fixture: &Fixture, root: &Path, args: Value) -> Value {
    let reply = bridge_raw(fixture, root, args, None);
    assert_eq!(reply["ok"], true, "{reply}");
    reply["result"].clone()
}
#[test]
fn whole_pipeline_reopens_resumes_and_graph_beats_serial() {
    let seed_root = root("seed");
    let project = seed(&seed_root);
    let (graph_root, serial_root) = (root("graph"), root("serial"));
    copy_tree(&seed_root, &graph_root);
    copy_tree(&seed_root, &serial_root);
    let mut results = Vec::new();
    let fixture = Fixture::start();
    for (folder, serial) in [(&graph_root, false), (&serial_root, true)] {
        fixture.reset();
        let at = Instant::now();
        let result = bridge(
            &fixture,
            folder,
            json!({"project_id":project,"serial":serial,"answers":serde_json::from_str::<Value>(ANSWERS).unwrap()}),
        );
        let wall = at.elapsed().as_secs_f64();
        let record = &result["record"];
        assert_eq!(record["status"], "completed", "{result}");
        let steps = record["steps"].as_array().unwrap();
        assert_eq!(
            steps
                .iter()
                .map(|s| s["step"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["hardening", "gate", "contracts", "phod", "product"]
        );
        assert!(steps.iter().all(|s| s["ended"].is_number()
            && s["result_id"].is_string()
            && s["status"] == "completed"));
        let hardening: Value = serde_json::from_slice(
            &fs::read(
                folder
                    .join("qroc/hardening")
                    .join(format!("{project}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(hardening["rounds"].as_array().unwrap().len(), 2);
        assert_eq!(hardening["rounds"][0]["questions"]["status"], "asked");
        assert!(hardening["rounds"][0]["answers"].is_string());
        assert_eq!(hardening["rounds"][1]["verdict"], "pass");
        let gates = &steps[1]["output"]["gates"];
        assert_eq!(gates.as_array().unwrap().len(), 3);
        assert_eq!(
            gates[0]["revise"]["latest"]["rounds"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(gates[0]["verdict"]["status"], "passed");
        assert_eq!(steps[3]["output"]["specs"][0]["invocation"]["exit_code"], 0);
        assert_eq!(
            steps[3]["output"]["specs"][0]["spec"]["dialect"],
            "phod-ir/1"
        );
        let spec_document = PathBuf::from(result["spec_document_path"].as_str().unwrap());
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(spec_document).unwrap()).unwrap(),
            steps[3]["output"]
        );
        assert_eq!(result["domains"]["PRODUCT"]["produced"], 16);
        for d in ["BUSINESS", "PRESENCE"] {
            assert_eq!(result["domains"][d]["state"], "predicted");
            assert_eq!(result["domains"][d]["produced"], 0);
        }
        let file = PathBuf::from(result["record_path"].as_str().unwrap());
        let before = fs::read(&file).unwrap();
        let projection = PathBuf::from(result["document_path"].as_str().unwrap());
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(projection).unwrap()).unwrap(),
            *record
        );
        let mut reopened = QrocStore::open(&folder.join("qroc")).unwrap();
        let docs = reopened
            .command(
                "qroc-document-list",
                &json!({"project_id":project,"branch_id":record["branch_id"]}),
            )
            .unwrap();
        assert_eq!(
            docs["documents"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["document_type"].as_str().unwrap())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            16
        );
        drop(reopened);
        let calls = fixture.calls.load(Ordering::SeqCst);
        let resumed = bridge(&fixture, folder, json!({"project_id":project}));
        assert_eq!(resumed["record"], *record);
        assert_eq!(fs::read(file).unwrap(), before);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), calls);
        assert_eq!(resumed["resumed"], true);
        println!("CHAIN_RECORD {}", record);
        println!("GATE_RECORDS {}", gates);
        println!("PHOD_RECORD {}", steps[3]["output"]);
        println!("DOMAIN_OVERVIEW {}", result["domains"]);
        println!(
            "MEASURE serial={serial} wall={wall:.3} peak={} overlap={:.3} calls={calls}",
            fixture.peak.load(Ordering::SeqCst),
            fixture.overlap()
        );
        results.push((wall, fixture.peak.load(Ordering::SeqCst), fixture.overlap()));
        // A completed journal cannot hide a subsequently failed current gate.
        let mut store = QrocStore::open(&folder.join("qroc")).unwrap();
        store.command("qroc-document-save",&json!({"project_id":project,"branch_id":record["branch_id"],"document_type":"GATE_3","source_revision":gates[2]["revision"],"content":"[SCORE]: 0\nCurrent gate no longer passes."})).unwrap();
        drop(store);
        let changed = bridge(&fixture, folder, json!({"project_id":project}));
        assert_eq!(changed["domains"]["PRODUCT"]["state"], "predicted");
        assert_eq!(changed["domains"]["PRODUCT"]["produced"], 0);
        assert_eq!(fixture.calls.load(Ordering::SeqCst), calls);
    }
    assert!(results[0].0 < results[1].0, "graph {results:?}");
    assert!(results[0].1 >= 2);
    assert_eq!(results[1].1, 1);
    assert!(results[0].2 > 0.2);
    assert_eq!(results[1].2, 0.0);
}
#[test]
fn hardening_waits_for_answers_then_cli_continues() {
    let folder = root("questions");
    let project = seed(&folder);
    let fixture = Fixture::start();
    let pending = bridge(&fixture, &folder, json!({"project_id":project}));
    assert_eq!(pending["record"]["status"], "waiting_answers");
    assert_eq!(pending["questions"].as_array().unwrap().len(), 1);
    let binary = probe_binary("qroc-run");
    let mut cmd = Command::new(binary);
    fixture.configure(&mut cmd);
    let mut child = cmd
        .args([
            "--root",
            folder.to_str().unwrap(),
            "--project",
            &project,
            "--from",
            "hardening",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{ANSWERS}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["record"]["status"], "completed");
    let calls = fixture.calls.load(Ordering::SeqCst);
    let mut cmd = Command::new(probe_binary("qroc-run"));
    fixture.configure(&mut cmd);
    let out = cmd
        .args([
            "--root",
            folder.to_str().unwrap(),
            "--project",
            &project,
            "--from",
            "product",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), calls);
}

#[test]
fn late_failure_resumes_completed_steps_and_from_cannot_skip() {
    let folder = root("late-failure");
    let project = seed(&folder);
    let fixture = Fixture::start();
    let premature = bridge_raw(
        &fixture,
        &folder,
        json!({"project_id":project,"from":"product"}),
        None,
    );
    assert_eq!(premature["ok"], false);
    assert!(premature["error"].as_str().unwrap().contains("Cannot skip"));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    let failed = bridge_raw(
        &fixture,
        &folder,
        json!({"project_id":project,"answers":serde_json::from_str::<Value>(ANSWERS).unwrap()}),
        Some("/no-such-phod-checker"),
    );
    assert_eq!(failed["ok"], false);
    let path = folder
        .join("qroc/pipeline")
        .join(&project)
        .join("Pipeline run.json");
    let before: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(before["status"], "failed");
    assert!(
        before["steps"].as_array().unwrap()[..3]
            .iter()
            .all(|s| s["status"] == "completed")
    );
    let calls = fixture.calls.load(Ordering::SeqCst);
    let bad = bridge_raw(
        &fixture,
        &folder,
        json!({"project_id":project,"from":"bogus"}),
        None,
    );
    assert_eq!(bad["ok"], false);
    assert_eq!(fixture.calls.load(Ordering::SeqCst), calls);
    let done = bridge(
        &fixture,
        &folder,
        json!({"project_id":project,"from":"phod"}),
    );
    assert_eq!(done["record"]["status"], "completed");
    assert_eq!(fixture.calls.load(Ordering::SeqCst), calls + 2);
    for i in 0..3 {
        assert_eq!(done["record"]["steps"][i], before["steps"][i]);
    }
    let changed = bridge_raw(
        &fixture,
        &folder,
        json!({"project_id":project,"serial":true}),
        None,
    );
    assert_eq!(changed["ok"], false);
    assert!(changed["error"].as_str().unwrap().contains("mode changed"));
    println!("RECOVERY preserved=3/3 new_route_calls=2/2 completed=5/5");
}

fn relay_table(folder: &Path, endpoint: &str, model: &str) {
    fs::write(folder.join("qroc/routes.json"), json!({"version":"qroc-routes/1","selected":"local-gateway","routes":[{"name":"local-gateway","kind":"relay","endpoint":endpoint,"model":model}]}).to_string()).unwrap();
}
#[test]
fn interrupted_revise_keeps_completed_document_and_resumes_same_round_after_port_change() {
    interruption_case(false);
}
#[test]
fn interrupted_second_round_migrates_legacy_route_without_new_call() {
    interruption_case(true);
}
fn interruption_case(second_round: bool) {
    let folder = root(if second_round {
        "interrupted-round2"
    } else {
        "interrupted"
    });
    let project = seed(&folder);
    let fixture = Fixture::start();
    fixture.fail_gates.store(second_round, Ordering::SeqCst);
    relay_table(&folder, &fixture.endpoint, "test");
    let mut press = None;
    if second_round {
        // Round 1 fails and the run stops: round 2 is his press, never the engine's.
        let waiting = bridge_raw(
            &fixture,
            &folder,
            json!({"project_id":project,"answers":serde_json::from_str::<Value>(ANSWERS).unwrap()}),
            None,
        );
        assert_eq!(waiting["ok"], false);
        assert!(
            waiting["error"].as_str().unwrap().contains(
                "GATE_1 failed round 1 of 2; the next round waits for the person's press"
            ),
            "{waiting}"
        );
        let journal: Value = serde_json::from_slice(
            &fs::read(
                folder
                    .join("qroc/pipeline")
                    .join(&project)
                    .join("Pipeline run.json"),
            )
            .unwrap(),
        )
        .unwrap();
        press = Some(
            json!({"version":1,"id":"press","command":"qroc-gate-revise-start",
            "args":{"project_id":project,"branch_id":journal["branch_id"],"gate":"GATE_1"}}),
        );
        fixture.fail_gates.store(false, Ordering::SeqCst);
    }
    let mut cmd = Command::new(probe_binary("qroc-bridge"));
    fixture.configure(&mut cmd);
    let mut child = cmd
        .args(["--root", folder.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    if let Some(press) = &press {
        writeln!(child.stdin.as_mut().unwrap(), "{press}").unwrap();
    } else {
        writeln!(child.stdin.as_mut().unwrap(),"{}",json!({"version":1,"id":"interrupt","command":"qroc-run","args":{"project_id":project,"answers":serde_json::from_str::<Value>(ANSWERS).unwrap()}})).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut polls = 0;
    let (checkpoint, completed_job) = loop {
        let mut found = None;
        if let Ok(projects) = fs::read_dir(folder.join("qroc/gate-revisions").join(&project)) {
            for branch in projects.flatten() {
                let path = branch.path().join("GATE_1.json");
                if let Ok(bytes) = fs::read(&path) {
                    let record: Value = serde_json::from_slice(&bytes).unwrap();
                    let round_index = usize::from(second_round);
                    if let Some(doc) = record["calls"][0]["rounds"][round_index]["documents"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|d| d["state"] == "succeeded")
                    {
                        found = Some((record.clone(), doc["job_id"].as_str().unwrap().to_owned()));
                    }
                }
            }
        }
        if let Some(value) = found {
            break value;
        }
        assert!(Instant::now() < deadline, "no completed checkpoint");
        if let Some(press) = &press {
            // Any command polls the engine; this drives his pressed round as the window does.
            let mut poll = press.clone();
            poll["command"] = json!("qroc-gate-revise-get");
            polls += 1;
            poll["id"] = json!(format!("poll-{polls}"));
            writeln!(child.stdin.as_mut().unwrap(), "{poll}").unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    };
    child.kill().unwrap();
    child.wait().unwrap();
    let job_path = folder
        .join("qroc/jobs")
        .join(format!("{completed_job}.json"));
    let before = fs::read(&job_path).unwrap();
    let job: Value = serde_json::from_slice(&before).unwrap();
    drop(fixture);
    if second_round {
        let journal = folder
            .join("qroc/pipeline")
            .join(&project)
            .join("Pipeline run.json");
        let mut record: Value = serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
        record["route"] = json!({"endpoint":"127.0.0.1:39429","model":"test"});
        fs::write(&journal, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    }
    let restarted = Fixture::start();
    // Do not force another low gate after restart; this tests transport, not a new call.
    restarted.gates.store(1, Ordering::SeqCst);
    relay_table(&folder, &restarted.endpoint, "test");
    let result = bridge(&restarted, &folder, json!({"project_id":project}));
    assert_eq!(result["record"]["status"], "completed");
    assert_eq!(result["record"]["route"]["kind"], "relay");
    assert!(result["record"]["route"].get("endpoint").is_none());
    assert_eq!(fs::read(&job_path).unwrap(), before);
    let branch = &result["record"]["branch_id"];
    let mut store = QrocStore::open(&folder.join("qroc")).unwrap();
    let doc=store.command("qroc-document-detail",&json!({"project_id":project,"branch_id":branch,"document_type":job["document_type"],"revision":checkpoint["calls"][0]["rounds"][usize::from(second_round)]["documents"][0]["revision"]})).unwrap();
    assert_eq!(doc["sha256"], job["output_sha256"]);
    let revise = store
        .command(
            "qroc-gate-revise-get",
            &json!({"project_id":project,"branch_id":branch,"gate":"GATE_1"}),
        )
        .unwrap();
    assert_eq!(revise["record"]["calls"].as_array().unwrap().len(), 1);
    assert_eq!(
        revise["latest"]["rounds"].as_array().unwrap().len(),
        if second_round { 2 } else { 1 }
    );
    assert_eq!(revise["latest"]["state"], "passed");
    let calls = restarted.calls.load(Ordering::SeqCst);
    drop(store);
    relay_table(&folder, &restarted.endpoint, "other-model");
    let changed = bridge_raw(&restarted, &folder, json!({"project_id":project}), None);
    assert_eq!(changed["ok"], false);
    assert!(
        changed["error"]
            .as_str()
            .unwrap()
            .contains("identity or route changed")
    );
    assert_eq!(restarted.calls.load(Ordering::SeqCst), calls);
    println!(
        "INTERRUPTION_CHECKPOINT job={} hash={} route={} call=1 preserved=1/1",
        completed_job, job["output_sha256"], result["record"]["route"]
    );
}
#[test]
fn failed_gate_keeps_product_predicted_and_preserves_two_round_limit() {
    let folder = root("failed-gate");
    let project = seed(&folder);
    let fixture = Fixture::start();
    fixture.fail_gates.store(true, Ordering::SeqCst);
    let failed = bridge_raw(
        &fixture,
        &folder,
        json!({"project_id":project,"answers":serde_json::from_str::<Value>(ANSWERS).unwrap()}),
        None,
    );
    assert_eq!(failed["ok"], false);
    let record: Value = serde_json::from_slice(
        &fs::read(
            folder
                .join("qroc/pipeline")
                .join(&project)
                .join("Pipeline run.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(record["domains"]["PRODUCT"]["state"], "predicted");
    assert_eq!(record["domains"]["PRODUCT"]["produced"], 0);
    assert!(
        record["steps"].as_array().unwrap()[2..]
            .iter()
            .all(|s| s["status"] == "waiting")
    );
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("the next round waits for the person's press"),
        "{failed}"
    );
    let before = fixture.calls.load(Ordering::SeqCst);
    let again = bridge_raw(&fixture, &folder, json!({"project_id":project}), None);
    assert_eq!(again["ok"], false);
    assert!(
        again["error"]
            .as_str()
            .unwrap()
            .contains("waits for the person's press")
    );
    assert_eq!(fixture.calls.load(Ordering::SeqCst), before);
    let mut store = QrocStore::open(&folder.join("qroc")).unwrap();
    let revise = store
        .command(
            "qroc-gate-revise-get",
            &json!({"project_id":project,"branch_id":record["branch_id"],"gate":"GATE_1"}),
        )
        .unwrap();
    assert_eq!(revise["max_rounds"], 2);
    assert_eq!(revise["latest"]["rounds"].as_array().unwrap().len(), 1);
    assert_eq!(revise["latest"]["state"], "failed");
    assert_eq!(revise["awaiting_press"], true);
    println!("FAILED_GATE_DOMAINS {}", record["domains"]);
}
#[test]
fn missing_budget_refuses_before_pipeline_journal_or_route_request() {
    let folder = root("budget");
    let project = seed(&folder);
    let fixture = Fixture::start();
    let mut cmd = Command::new(probe_binary("qroc-bridge"));
    fixture.configure(&mut cmd);
    cmd.env_remove("QROC_MAX_TOKENS");
    let mut child = cmd
        .args(["--root", folder.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        json!({"version":1,"id":"budget","command":"qroc-run","args":{"project_id":project}})
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    let reply: Value = serde_json::from_str(
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .nth(1)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap().contains("QROC_MAX_TOKENS"));
    assert!(!folder.join("qroc/pipeline").join(&project).exists());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 0);
    println!("EARLY_BUDGET_REFUSAL {} requests=0 journals=0", reply);
}
