use qroc_standalone::{
    bridge::Bridge,
    intake,
    qroc::QrocStore,
    routes::{Kind, Route, RouteTable},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn root(name: &str) -> PathBuf {
    let p =
        PathBuf::from(std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"))
            .join("intake-tests")
            .join(format!(
                "intake-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
    fs::create_dir_all(&p).unwrap();
    p
}
fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/intake")
        .join(name)
        .to_string_lossy()
        .into_owned()
}
fn project(root: &Path) -> String {
    QrocStore::open(root)
        .unwrap()
        .command(
            "qroc-project-create",
            &json!({"title":"Phone capture", "brain_dump":"Original idea"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn check_record(v: &Value, kind: &str, source: &str) {
    let d = &v["document"];
    assert_eq!(d["kind"], kind);
    assert_eq!(d["source"], source);
    assert_eq!(d["sha256"].as_str().unwrap().len(), 64);
    assert!(d["received_at"].as_u64().unwrap() > 0);
    println!("RECORD {}", d);
}
#[test]
fn note_becomes_inbox_document_with_provenance() {
    let r = root("note");
    let p = project(&r);
    let path = fixture("note.txt");
    let v = intake::intake(&r, &p, &path, "note").unwrap();
    check_record(&v, "note", &path);
    assert!(
        v["document"]["content"]
            .as_str()
            .unwrap()
            .contains("neighbours")
    );
    assert!(Path::new(v["document"]["path"].as_str().unwrap()).starts_with(r.join("inbox")));
    let kb = QrocStore::open(&r)
        .unwrap()
        .command("qroc-kb-list", &json!({"project_id":p}))
        .unwrap();
    assert_eq!(kb["items"].as_array().unwrap().len(), 1);
}
#[test]
fn screenshot_becomes_inbox_document_with_provenance() {
    let r = root("png");
    let p = project(&r);
    let path = fixture("screenshot.png");
    check_record(
        &intake::intake(&r, &p, &path, "screenshot").unwrap(),
        "screenshot",
        &path,
    );
}
#[test]
fn voice_carries_recognizer_transcript() {
    let r = root("wav");
    let p = project(&r);
    let path = fixture("short.wav");
    let v = intake::intake(&r, &p, &path, "voice").unwrap();
    check_record(&v, "voice", &path);
    assert_eq!(v["document"]["recognizer"], "voxtype-cpu");
    assert!(v["document"]["transcript"].is_string());
    assert!(
        !v["document"]["transcript"]
            .as_str()
            .unwrap()
            .contains("Loading audio file:")
    );
    assert!(Path::new(v["document"]["transcript_path"].as_str().unwrap()).is_file());
}
#[test]
fn link_becomes_inbox_document_with_provenance() {
    let r = root("link");
    let p = project(&r);
    let url = format!("file://{}", video());
    check_record(&intake::intake(&r, &p, &url, "link").unwrap(), "link", &url);
}
#[test]
fn duplicate_sha256_adds_no_document() {
    let r = root("dup");
    let p = project(&r);
    let path = fixture("note.txt");
    let a = intake::intake(&r, &p, &path, "note").unwrap();
    let b = intake::intake(&r, &p, &path, "note").unwrap();
    assert_eq!(a["document"], b["document"]);
    assert_eq!(b["duplicate"], true);
    let n = fs::read_dir(r.join("inbox").join(&p))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "json")
        })
        .count();
    assert_eq!(n, 1);
}
fn video() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/intake/download.mp4");
    assert!(p.is_file(), "video fixture generated before tests");
    p.to_string_lossy().into_owned()
}
#[test]
fn local_download_fixture_has_transcript_and_frames() {
    let r = root("video");
    let p = project(&r);
    let url = format!("file://{}", video());
    let v = intake::intake(&r, &p, &url, "link").unwrap();
    assert_eq!(
        v["document"]["analysis"]["status"],
        "held: no vision route configured"
    );
    assert_eq!(v["document"]["frames"].as_array().unwrap().len(), 2);
    assert!(v["document"]["transcript"].is_string());
    assert!(Path::new(v["document"]["download"].as_str().unwrap()).is_file());
    println!("CHAIN {}", v);
}
fn fake_route(root: &Path, replies: Vec<String>) -> thread::JoinHandle<Vec<Value>> {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    l.set_nonblocking(true).unwrap();
    let mut t = RouteTable::open_with(root, &|_| None).unwrap();
    t.add(Route {
        name: "fixture".into(),
        kind: Kind::Local,
        endpoint: addr,
        model: "fixture".into(),
        key_ref: None,
    })
    .unwrap();
    t.select("fixture").unwrap();
    thread::spawn(move || {
        let mut requests = Vec::new();
        for content in replies {
            let end = std::time::Instant::now() + Duration::from_secs(60);
            let (mut s, _) = loop {
                match l.accept() {
                    Ok(v) => break v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < end, "route request timed out");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut reader = BufReader::new(s.try_clone().unwrap());
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
            requests.push(serde_json::from_slice(&bytes).unwrap());
            let body=json!({"model":"fixture","choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string();
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        requests
    })
}
#[test]
fn start_hardening_uses_intake_note_on_stand_in_route() {
    let r = root("harden").join("qroc");
    let p = project(&r);
    let server = fake_route(
        &r,
        vec![
            json!({"answers":[{"index":0,"score":30,"reason":"Need clearer customer evidence"}]})
                .to_string(),
            "- Which neighbour will test the first prototype?".into(),
        ],
    );
    let begin = std::time::Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_qroc-intake"))
        .args([
            "--root",
            r.to_str().unwrap(),
            "--project",
            &p,
            "note",
            "--text",
            "Help neighbours share tools",
            "--start-hardening",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["hardening"]["rounds"][0]["questions"]["items"][0],
        "Which neighbour will test the first prototype?"
    );
    println!(
        "HARDENING {} wall_ms={}",
        v["hardening"],
        begin.elapsed().as_millis()
    );
    let req = server.join().unwrap();
    assert!(req[0].to_string().contains("Help neighbours share tools"));
}
#[test]
fn configured_vision_route_receives_frame_images() {
    let r = root("vision");
    let p = project(&r);
    let server = fake_route(&r, vec!["Two frames of the local fixture".into()]);
    let v = intake::intake(&r, &p, &format!("file://{}", video()), "link").unwrap();
    assert_eq!(
        v["document"]["analysis"]["status"],
        "held: route did not attach the frames"
    );
    let req = server.join().unwrap();
    let items = req[0]["messages"][0]["content"].as_array().unwrap();
    assert_eq!(items.iter().filter(|i| i["type"] == "image_url").count(), 2);
    assert!(
        items[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
}
#[test]
fn binary_and_bridge_return_identical_note_replies() {
    let r = root("parity");
    let qr = r.join("qroc");
    let p = project(&qr);
    let text = "Parity phone note";
    intake::intake(&qr, &p, text, "text").unwrap();
    let a = Command::new(env!("CARGO_BIN_EXE_qroc-intake"))
        .args([
            "--root",
            qr.to_str().unwrap(),
            "--project",
            &p,
            "note",
            "--text",
            text,
        ])
        .output()
        .unwrap();
    assert!(a.status.success());
    let a: Value = serde_json::from_slice(&a.stdout).unwrap();
    let b=Bridge::open(&r).unwrap().reply(json!({"version":1,"id":"parity","command":"qroc-intake","args":{"project_id":p,"kind":"text","source":text}}));
    assert_eq!(b["ok"], true, "{b}");
    assert_eq!(a, b["result"]);
    println!("BINARY {a}\nBRIDGE {b}");
}
#[test]
fn invalid_project_and_traversal_rejected() {
    let r = root("bad");
    assert!(intake::intake(&r, "../escape", "text", "text").is_err());
    assert!(intake::intake(&r, "missing", "text", "text").is_err());
}

#[test]
fn source_inside_root_is_referenced_and_survives_reopen() {
    let r = root("reference");
    let p = project(&r);
    let source = r.join("captured.txt");
    fs::write(&source, "A persistent local note").unwrap();
    let a = intake::intake(&r, &p, source.to_str().unwrap(), "note").unwrap();
    assert_eq!(a["document"]["path"], source.to_str().unwrap());
    let b = QrocStore::open(&r)
        .unwrap()
        .command("qroc-intake", &json!({"project_id":p,"path":source}))
        .unwrap();
    assert_eq!(a["document"], b["document"]);
    assert_eq!(b["duplicate"], true);
}

#[test]
fn relative_root_is_rejected_before_creating_state() {
    assert!(
        intake::intake(
            Path::new("relative-root-must-not-exist"),
            "p",
            "note",
            "text"
        )
        .is_err()
    );
    assert!(!Path::new("relative-root-must-not-exist").exists());
}

#[test]
fn leftover_lock_file_does_not_block_intake() {
    let r = root("lock");
    let p = project(&r);
    let dir = r.join("inbox").join(&p);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".intake.lock"), "").unwrap();
    assert!(intake::intake(&r, &p, "A note after a previous process ended", "text").is_ok());
}

#[test]
fn automatic_hardening_refuses_cloud_route() {
    let r = root("cloud");
    let p = project(&r);
    let mut table = RouteTable::open_with(&r, &|_| None).unwrap();
    table
        .add(Route {
            name: "cloud".into(),
            kind: Kind::Openrouter,
            endpoint: "https://openrouter.ai/api/v1/chat/completions".into(),
            model: "fixture".into(),
            key_ref: Some("QROC_TEST_UNUSED_KEY".into()),
        })
        .unwrap();
    table.select("cloud").unwrap();
    let error = QrocStore::open(&r)
        .unwrap()
        .command(
            "qroc-intake",
            &json!({"project_id":p,"text":"Keep source local","start_hardening":true}),
        )
        .unwrap_err();
    assert!(error.to_string().contains("requires a local route"));
    assert!(!r.join("receipts.jsonl").exists());
}

#[test]
fn full_stdin_and_root_forms_reopen_distinct_named_projects() {
    let container = root("full-stdin");
    let store_root = container.join("qroc");
    let mut store = QrocStore::open(&store_root).unwrap();
    let a = store
        .command(
            "qroc-project-create",
            &json!({"title":"Full multiline idea", "brain_dump":"Original project idea"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = store
        .command(
            "qroc-project-create",
            &json!({"title":"Source link project", "brain_dump":"Original source project"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    drop(store);
    let text = "  First line\r\n\nSecond line: café 🛠\nlast line\n";
    let args = vec![
        "--root".into(),
        container.display().to_string(),
        "--project".into(),
        a.clone(),
        "note".into(),
    ];
    let note = intake::cli_with_input(&args, &mut text.as_bytes()).unwrap();
    assert_eq!(note["document"]["content"], text);
    let url = format!("file://{}", video());
    let link = intake::cli(&[
        "--root".into(),
        store_root.display().to_string(),
        "--project".into(),
        b.clone(),
        "link".into(),
        "--url".into(),
        url.clone(),
    ])
    .unwrap();
    assert_eq!(link["document"]["source"], url);
    let mut reopened = QrocStore::open(&store_root).unwrap();
    for (id, content) in [(&a, text), (&b, url.as_str())] {
        let kb = reopened
            .command("qroc-kb-list", &json!({"project_id":id}))
            .unwrap();
        assert_eq!(kb["items"].as_array().unwrap().len(), 1);
        assert!(
            kb["items"][0]["content"]
                .as_str()
                .unwrap()
                .starts_with(content)
        );
    }
    assert_eq!(intake::container_root(&container).unwrap(), container);
    assert_eq!(intake::container_root(&store_root).unwrap(), container);
    assert_eq!(
        intake::container_root(&store_root.join(".")).unwrap(),
        container
    );
    assert!(intake::container_root(Path::new("relative")).is_err());
    assert!(intake::container_root(&container.join("../escape")).is_err());
    let mut invalid = args.clone();
    invalid.push("--stdin".into());
    assert!(intake::cli_with_input(&invalid, &mut &[0xff][..]).is_err());
    invalid.extend(["--text".into(), "conflict".into()]);
    assert!(intake::cli_with_input(&invalid, &mut text.as_bytes()).is_err());
}

#[test]
fn failed_fetch_is_an_error_without_inbox_success_or_hardening() {
    let container = root("failed-fetch");
    let qr = container.join("qroc");
    let p = project(&qr);
    let tools = container.join("tools");
    fs::create_dir_all(&tools).unwrap();
    let downloader = tools.join("yt-dlp");
    fs::write(
        &downloader,
        "#!/bin/sh\necho 'controlled fetch failure' >&2\nexit 1\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&downloader, fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_qroc-intake"))
        .env("PATH", &tools)
        .args([
            "--root",
            container.to_str().unwrap(),
            "--project",
            &p,
            "link",
            "--url",
            "https://example.invalid/unreachable",
            "--start-hardening",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let error: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("could not be fetched")
    );
    assert!(!qr.join("hardening").join(format!("{p}.json")).exists());
    assert!(
        !fs::read_dir(qr.join("inbox").join(&p)).unwrap().any(|e| e
            .unwrap()
            .path()
            .extension()
            .is_some_and(|x| x == "json"))
    );
    let kb = QrocStore::open(&qr)
        .unwrap()
        .command("qroc-kb-list", &json!({"project_id":p}))
        .unwrap();
    assert!(kb["items"].as_array().unwrap().is_empty());
}

#[test]
fn legacy_unfetched_link_does_not_report_duplicate_success() {
    use sha2::{Digest, Sha256};
    let r = root("legacy-link");
    let p = project(&r);
    let url = "https://example.invalid/legacy-video";
    let hash = format!("{:x}", Sha256::digest(url.as_bytes()));
    let dir = r.join("inbox").join(&p);
    fs::create_dir_all(&dir).unwrap();
    let record = dir.join(format!("{hash}.json"));
    let bytes = serde_json::to_vec(
        &json!({"project_id":p,"source":url,"download_status":"held: download failed"}),
    )
    .unwrap();
    fs::write(&record, &bytes).unwrap();
    let error = intake::intake(&r, &p, url, "link").unwrap_err();
    assert!(error.to_string().contains("was not fetched"));
    assert_eq!(fs::read(&record).unwrap(), bytes);
}
