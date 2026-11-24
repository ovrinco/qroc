//! A project of many rounds is listed completely across pages; a small project keeps today's reply.
#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{
    bridge::{Bridge, MAX_FRAME, MAX_REPLY},
    qroc::QrocStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

fn root(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(format!("scratch-paging-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

struct Wire {
    bridge: Bridge,
    n: usize,
}
impl Wire {
    fn ask(&mut self, command: &str, args: Value) -> Value {
        self.n += 1;
        let reply = self
            .bridge
            .reply(json!({"version":1,"id":format!("r{}",self.n),"command":command,"args":args}));
        assert_eq!(reply["ok"], true, "{command}: {}", reply["error"]);
        let size = serde_json::to_vec(&reply).unwrap().len();
        assert!(size < MAX_FRAME, "{command} reply is {size} bytes");
        reply["result"].clone()
    }
}

/// A project and branch with `per_type` revisions of each of five document types.
fn project(root: &Path, per_type: usize, body: usize) -> (Wire, String, String) {
    let mut wire = Wire {
        bridge: Bridge::open(root).unwrap(),
        n: 0,
    };
    let p = wire.ask(
        "qroc-project-create",
        json!({"title":"Many rounds","brain_dump":"A project revised over many gate rounds."}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    hardening::pass_hardening(&root.join("qroc"));
    let b = wire.ask(
        "qroc-branch-create",
        json!({"project_id":p,"name":"rounds"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for kind in [
        "STRATEGY_BRIEF",
        "PRE_MORTEM",
        "PRD",
        "TECHNICAL_ARCHITECTURE",
        "GATE_1",
    ] {
        for r in 0..per_type {
            let content = format!("{kind} round {r}\n{}", "x".repeat(body));
            let mut args =
                json!({"project_id":p,"branch_id":b,"document_type":kind,"content":content});
            if r > 0 {
                args["source_revision"] = json!(r);
            }
            wire.ask("qroc-document-save", args);
        }
    }
    (wire, p, b)
}

/// Every page from the first, following `next_offset`; returns the items in order and the page count.
fn all_pages(wire: &mut Wire, command: &str, args: Value, key: &str) -> (Vec<Value>, usize) {
    let (mut items, mut pages, mut offset) = (Vec::new(), 0, 0);
    loop {
        let mut a = args.clone();
        a["offset"] = json!(offset);
        let page = wire.ask(command, a);
        pages += 1;
        assert_eq!(page["offset"], offset);
        let got = page[key].as_array().unwrap();
        assert!(!got.is_empty() || page["total"] == 0);
        items.extend(got.iter().cloned());
        assert_eq!(page["next_offset"], items.len());
        if page["more"] == false {
            assert_eq!(page["total"], items.len());
            return (items, pages);
        }
        offset = page["next_offset"].as_u64().unwrap();
    }
}

fn ids(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn forty_large_documents_are_listed_completely_across_pages() {
    let r = root("documents");
    let (mut wire, p, b) = project(&r, 8, 700 * 1024);
    let owner = json!({"project_id":p,"branch_id":b});
    // The window's plain request is bounded and says there is more.
    let first = wire.ask("qroc-document-list", owner.clone());
    assert_eq!(first["total"], 40);
    assert_eq!(first["more"], true);
    assert!(first["documents"].as_array().unwrap().len() < 40);
    assert!(first["domains"].is_array() && first["document_order"].is_object());
    let (docs, pages) = all_pages(&mut wire, "qroc-document-list", owner.clone(), "documents");
    assert!(pages > 1);
    assert_eq!(docs.len(), 40);
    assert_eq!(ids(&docs).into_iter().collect::<BTreeSet<_>>().len(), 40);
    // Pages concatenate to the engine's own whole list, in its order.
    drop(wire);
    let mut store = QrocStore::open(&r.join("qroc")).unwrap();
    let whole = store.command("qroc-document-list", &owner).unwrap();
    assert!(whole.get("total").is_none());
    assert_eq!(ids(whole["documents"].as_array().unwrap()), ids(&docs));
    drop(store);
    let mut wire = Wire {
        bridge: Bridge::open(&r).unwrap(),
        n: 100,
    };
    let three = wire.ask(
        "qroc-document-list",
        json!({"project_id":p,"branch_id":b,"offset":2,"limit":3}),
    );
    // At most `limit` items, and fewer when the byte budget ends the page first.
    let got = three["documents"].as_array().unwrap();
    assert!((1..=3).contains(&got.len()));
    assert_eq!(ids(got), ids(&docs[2..2 + got.len()]));
    assert_eq!(three["next_offset"], 2 + got.len());
    assert_eq!(three["more"], true);
    let small = wire.ask(
        "qroc-document-history",
        json!({"project_id":p,"branch_id":b,"document_type":"PRD","limit":1}),
    );
    assert_eq!(small["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        (small["total"].clone(), small["next_offset"].clone()),
        (json!(8), json!(1))
    );
    // A gate's revision history pages the same way.
    let gate = json!({"project_id":p,"branch_id":b,"document_type":"GATE_1"});
    let (history, _) = all_pages(&mut wire, "qroc-document-history", gate, "items");
    let revisions = history
        .iter()
        .map(|d| d["revision"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(revisions, (1..=8).collect::<Vec<_>>());
    // Past the end: an empty last page.
    let past = wire.ask(
        "qroc-document-list",
        json!({"project_id":p,"branch_id":b,"offset":99}),
    );
    assert_eq!(past["documents"], json!([]));
    assert_eq!(
        (past["total"].clone(), past["more"].clone()),
        (json!(40), json!(false))
    );
    let bad = wire
        .bridge
        .reply(json!({"version":1,"id":"bad","command":"qroc-job-list","args":{"limit":0}}));
    assert_eq!(bad["ok"], false);
    let _ = fs::remove_dir_all(r);
}

#[test]
fn forty_large_jobs_are_listed_completely_across_pages() {
    let r = root("jobs");
    let jobs = r.join("qroc/jobs");
    fs::create_dir_all(&jobs).unwrap();
    for i in 0..40 {
        let id = format!("qroc-job-1790000000-{i:04}");
        let job = json!({"id":id,"project_id":"qroc-project-1790000000-0001","branch_id":"qroc-branch-1790000000-0002",
            "document_type":"PRD","state":"completed","model":"fixture","queued_context":"c".repeat(900 * 1024),
            "error":null,"document_id":null,"started_at":1790000000,"finished_at":1790000001});
        fs::write(jobs.join(format!("{id}.json")), job.to_string()).unwrap();
    }
    let mut wire = Wire {
        bridge: Bridge::open(&r).unwrap(),
        n: 0,
    };
    let owner = json!({"project_id":"qroc-project-1790000000-0001"});
    let first = wire.ask("qroc-job-list", owner.clone());
    assert_eq!(
        (first["total"].clone(), first["more"].clone()),
        (json!(40), json!(true))
    );
    let (all, _) = all_pages(&mut wire, "qroc-job-list", owner, "jobs");
    assert_eq!(ids(&all).into_iter().collect::<BTreeSet<_>>().len(), 40);
    let none = wire.ask("qroc-job-list", json!({"project_id":"qroc-project-other"}));
    assert_eq!(none, json!({"jobs":[]}));
    let _ = fs::remove_dir_all(r);
}

#[test]
fn small_project_reply_is_today_shape_and_store_equal() {
    let r = root("small");
    let (mut wire, p, b) = project(&r, 2, 64);
    let owner = json!({"project_id":p,"branch_id":b});
    let gate = json!({"project_id":p,"branch_id":b,"document_type":"GATE_1"});
    let wire_docs = wire.ask("qroc-document-list", owner.clone());
    let wire_hist = wire.ask("qroc-document-history", gate.clone());
    let wire_jobs = wire.ask("qroc-job-list", json!({"project_id":p}));
    let paged = wire.ask(
        "qroc-document-list",
        json!({"project_id":p,"branch_id":b,"offset":0}),
    );
    drop(wire);
    let mut store = QrocStore::open(&r.join("qroc")).unwrap();
    let docs = store.command("qroc-document-list", &owner).unwrap();
    assert_eq!(
        serde_json::to_vec(&wire_docs).unwrap(),
        serde_json::to_vec(&docs).unwrap()
    );
    assert_eq!(
        wire_docs.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["document_order", "documents", "domains"]
    );
    assert_eq!(
        wire_hist,
        store.command("qroc-document-history", &gate).unwrap()
    );
    assert_eq!(
        wire_jobs,
        store
            .command("qroc-job-list", &json!({"project_id":p}))
            .unwrap()
    );
    assert_eq!(paged["documents"], docs["documents"]);
    assert_eq!(
        (paged["total"].clone(), paged["more"].clone()),
        (json!(10), json!(false))
    );
    let _ = fs::remove_dir_all(r);
}

#[test]
fn oversized_reply_is_refused_in_plain_words_and_service_keeps_running() {
    let r = root("oversized");
    let mut bridge = Bridge::open(&r).unwrap();
    // Project list is not paged: nine 1.9 MiB brain dumps make one reply over 16 MiB.
    for i in 0..9 {
        let reply = bridge.reply(
            json!({"version":1,"id":format!("p{i}"),"command":"qroc-project-create",
            "args":{"title":format!("Big {i}"),"brain_dump":"b".repeat(1900 * 1024)}}),
        );
        assert_eq!(reply["ok"], true);
    }
    let input = "{\"version\":1,\"id\":\"big\",\"command\":\"qroc-project-list\",\"args\":{}}\n\
                 {\"version\":1,\"id\":\"after\",\"command\":\"bridge-capabilities\",\"args\":{}}\n";
    let mut out = Vec::new();
    bridge.serve(Cursor::new(input), &mut out).unwrap();
    let lines = out
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l.len() <= MAX_REPLY));
    let big: Value = serde_json::from_slice(lines[1]).unwrap();
    assert_eq!(
        (big["id"].clone(), big["ok"].clone()),
        (json!("big"), json!(false))
    );
    let error = big["error"].as_str().unwrap();
    assert!(
        error.starts_with("The reply to qroc-project-list for the whole reply is "),
        "{error}"
    );
    assert!(
        error.ends_with("MiB, over the bridge's 16 MiB limit"),
        "{error}"
    );
    let after: Value = serde_json::from_slice(lines[2]).unwrap();
    assert_eq!(
        (after["id"].clone(), after["ok"].clone()),
        (json!("after"), json!(true))
    );
    let _ = fs::remove_dir_all(r);
}
