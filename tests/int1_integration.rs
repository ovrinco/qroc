//! Int-1 wiring: the routes table drives generation, the hardening gate guards a graph batch,
//! and the Product tail runs T404's steps with one spec and one verification per contract.
#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{
    bridge::Bridge,
    contracts, harden,
    qroc::QrocStore,
    routes::{Kind, Route, RouteTable},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

fn root(name: &str) -> PathBuf {
    let p = std::path::PathBuf::from(
        std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"),
    )
    .join(format!(
        "int1-{}-{name}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

/// A loopback stand-in for a route: answers each request in turn with the given content.
fn fake_route(replies: Vec<String>) -> (String, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for content in replies {
            let (mut stream, _) = listener.accept().unwrap();
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
            requests.push(serde_json::from_slice(&bytes).unwrap());
            let body = json!({"model":"fixture","choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        requests
    });
    (endpoint, server)
}

fn select(qroc_root: &std::path::Path, route: Route) {
    let mut table = RouteTable::open_with(qroc_root, &|_| None).unwrap();
    let name = route.name.clone();
    table.add(route).unwrap();
    table.select(&name).unwrap();
}

#[test]
fn selected_route_in_the_table_drives_generation_and_hardening() {
    let p = root("routes");
    let mut bridge = Bridge::open(&p).unwrap();
    let qroc_root = p.join("qroc");
    select(
        &qroc_root,
        Route {
            name: "picked-relay".into(),
            kind: Kind::Relay,
            endpoint: "127.0.0.1:9".into(),
            model: "codex:fixture".into(),
            key_ref: None,
        },
    );
    let cap = bridge.reply(
        json!({"version":1,"id":"cap","command":"qroc-registry-provider-capabilities","args":{}}),
    );
    assert_eq!(cap["result"]["provider"], "picked-relay", "{cap}");
    assert_eq!(cap["result"]["endpoint"], "127.0.0.1:9");
    assert_eq!(cap["result"]["model"], "codex:fixture");
    let list =
        bridge.reply(json!({"version":1,"id":"list","command":"qroc-routes-list","args":{}}));
    assert_eq!(
        list["result"]["routes"][0]["name"], "picked-relay",
        "{list}"
    );
    assert_eq!(list["result"]["routes"][0]["selected"], true);
    // The window names a relay route by the model it carries (codex:, apikey:, anthropic:), so the list carries it.
    assert_eq!(
        list["result"]["routes"][0]["model"], "codex:fixture",
        "{list}"
    );
    // Hardening asks the same route: the table's endpoint is tried, not "no route declared".
    let q = harden::ask_questions(&qroc_root, "An idea", &harden::score("An idea"));
    assert_eq!(q.status, "unavailable");
    assert!(
        q.reason.as_deref().unwrap_or("").contains("127.0.0.1:9"),
        "{q:?}"
    );
    // An OpenRouter selection is reported, never sent to the loopback generator.
    select(
        &qroc_root,
        Route {
            name: "openrouter".into(),
            kind: Kind::Openrouter,
            endpoint: "https://openrouter.ai/api/v1/chat/completions".into(),
            model: "fixture/model".into(),
            key_ref: Some("QROC_FIXTURE_KEY".into()),
        },
    );
    let cap = bridge.reply(
        json!({"version":1,"id":"cap2","command":"qroc-registry-provider-capabilities","args":{}}),
    );
    assert_eq!(cap["result"]["available"], false, "{cap}");
    assert!(
        cap["result"]["reason"]
            .as_str()
            .unwrap()
            .contains("OpenRouter")
    );
}

#[test]
fn graph_batch_is_refused_before_hardening_passes() {
    let p = root("gate");
    let mut store = QrocStore::open(&p.join("qroc")).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Gate","brain_dump":"A tool that makes slides."}),
        )
        .unwrap();
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project["id"],"name":"Main"}),
        )
        .unwrap();
    let result = store.command(
        "qroc-batch-start",
        &json!({"project_id":project["id"],"branch_id":branch["id"],"document_types":["STRATEGY_BRIEF","PRD"],"mode":"graph"}),
    );
    let error = result.unwrap_err().to_string();
    assert!(error.contains("Hardening gate has not passed"), "{error}");
    assert_eq!(fs::read_dir(p.join("qroc/jobs")).unwrap().count(), 0);
}

#[test]
fn tail_runs_contracts_then_one_spec_and_verification_per_contract_then_the_join() {
    let p = root("tail");
    let qroc_root = p.join("qroc");
    let mut store = QrocStore::open(&qroc_root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Tail fixture","brain_dump":"Fixture idea"}),
        )
        .unwrap();
    hardening::pass_hardening(&qroc_root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project["id"],"name":"Main"}),
        )
        .unwrap();
    let (project_id, branch_id) = (
        project["id"].as_str().unwrap().to_owned(),
        branch["id"].as_str().unwrap().to_owned(),
    );
    for (kind, content) in [
        ("TECHNICAL_ARCHITECTURE", "Fixture architecture"),
        ("SLICE_INDEX", "Fixture slice index"),
    ] {
        store.command("qroc-document-save", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":kind,"content":content})).unwrap();
    }
    let early = store.tail_input(&project_id, &branch_id).unwrap_err();
    assert!(early.to_string().contains("Gate 3"), "{early}");
    store.command("qroc-document-save", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"GATE_3","content":"Fixture only.\n[SCORE]: 95\n- Epic Coverage: 29/30 - fixture\n- Slice Independence: 24/25 - fixture\n- Slice Acceptance: 23/25 - fixture\n- Traceability: 19/20 - fixture"})).unwrap();
    let input = store.tail_input(&project_id, &branch_id).unwrap();
    let contracts_reply = json!([
        {"name":"Scene metadata","purpose":"Inspect scene metadata","inputs":["record"],"outputs":["validity"],
         "behaviour":["nonempty text: title","integer: slides in 1..100","unknown fields: reject"],
         "errors":["reject invalid record"],"effects":[],"profile":"qroc-record/1",
         "record":{"text_field":"title","integer_field":"slides","minimum":1,"maximum":100,"unknown_fields":"reject"},
         "unsupported_reason":null},
        {"name":"Deck writer","purpose":"Write the deck","inputs":["scene"],"outputs":["deck"],
         "behaviour":["compile scene"],"errors":["reject bad scene"],"effects":["write deck"],"profile":"unsupported",
         "record":null,"unsupported_reason":"writing files is outside the pure record profile"}
    ]);
    let (endpoint, server) = fake_route(vec![
        contracts_reply.to_string(),
        "Ready for the record check; the deck writer stays outside PHOD.".into(),
    ]);
    select(
        &qroc_root,
        Route {
            name: "fixture-route".into(),
            kind: Kind::Local,
            endpoint,
            model: "fixture".into(),
            key_ref: None,
        },
    );
    let result = contracts::run_tail(&qroc_root, &input).unwrap();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(result["specs"].as_array().unwrap().len(), 1);
    assert_eq!(result["unsupported"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["verifications"][0]["verification"]["status"],
        "verified"
    );
    assert_eq!(result["join"]["verified"], 1);
    let graph = store
        .graph_export(&json!({"project_id":project_id,"branch_id":branch_id}))
        .unwrap();
    let nodes = graph["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 21);
    for kind in contracts::TAIL_NODES {
        let node = nodes.iter().find(|n| n["id"] == kind).unwrap();
        assert_eq!(node["state"], "produced", "{node}");
        assert!(node["step"].as_str().unwrap().starts_with("contracts:"));
    }
    let spec_node = nodes.iter().find(|n| n["id"] == "PHOD_SPEC").unwrap();
    assert_eq!(spec_node["instances"].as_array().unwrap().len(), 2);
    let folder = store.open_project_folder(&project_id).unwrap();
    for file in [
        "idea/Hardening.md",
        "product/contracts/Scene metadata.md",
        "product/contracts/Deck writer.md",
        "product/specs/Scene metadata.md",
        "product/specs/Deck writer.md",
        "product/Consistency join.md",
    ] {
        assert!(folder.join(file).is_file(), "{file}");
    }
    assert!(
        fs::read_to_string(folder.join("product/specs/Deck writer.md"))
            .unwrap()
            .contains("Unsupported: writing files")
    );
}
