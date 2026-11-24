//! Fixture project (labelled fixture): three Component contracts handed to a work table and
//! joined back. No route, model or worker is started.
#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{contracts, qroc::QrocStore, worktable};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn fixture_root(label: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(format!("fixtures-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn record(name: &str, text: &str, int: &str) -> Value {
    json!({"name":name,"purpose":format!("Fixture: inspect {name}"),"inputs":["record"],"outputs":["validity"],
        "behaviour":[format!("nonempty text: {text}"),format!("integer: {int} in 1..100"),"unknown fields: reject"],
        "errors":["reject invalid record"],"effects":[],"profile":"qroc-record/1",
        "record":{"text_field":text,"integer_field":int,"minimum":1,"maximum":100,"unknown_fields":"reject"},
        "unsupported_reason":null})
}

/// A store with one project and branch whose tail revision r1 holds three fixture contracts.
fn fixture(label: &str) -> (PathBuf, QrocStore, String, String) {
    let root = fixture_root(label);
    let mut store = QrocStore::open(&root).unwrap();
    let p = store
        .command(
            "qroc-project-create",
            &json!({"title":format!("Fixture work table {label}"),"brain_dump":"Fixture idea only"}),
        )
        .unwrap();
    let project = p["id"].as_str().unwrap().to_owned();
    hardening::pass_hardening(&root);
    let b = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project,"name":"Fixture branch"}),
        )
        .unwrap();
    let branch = b["id"].as_str().unwrap().to_owned();
    let dir = root.join("tail").join(&project).join(&branch).join("r1");
    fs::create_dir_all(dir.join("contracts")).unwrap();
    let deck = json!({"name":"Deck renderer","purpose":"Fixture: render slides","inputs":["scene"],"outputs":["deck"],
        "behaviour":["render slides"],"errors":["reject bad scene"],"effects":["write deck file"],"profile":"unsupported",
        "record":null,"unsupported_reason":"deck rendering writes files; qroc-record/1 is pure inspection"});
    for (i, c) in [
        record("Scene metadata", "title", "slides"),
        record("Speaker note", "text", "seconds"),
        deck,
    ]
    .iter()
    .enumerate()
    {
        fs::write(
            dir.join(format!("contracts/component-{i}.json")),
            json!({"revision":1,"contract":c}).to_string(),
        )
        .unwrap();
    }
    (root, store, project, branch)
}

/// What a worker writes: the spec derived from its contract, or the unsupported reason.
fn worker_result(row_dir: &std::path::Path) -> Value {
    let input: Value =
        serde_json::from_slice(&fs::read(row_dir.join("contract.json")).unwrap()).unwrap();
    let c: contracts::Contract = serde_json::from_value(input["contract"].clone()).unwrap();
    let base = json!({"schema":worktable::RESULT_SCHEMA,"row":input["row"],"contract_sha256":input["contract_sha256"]});
    let mut r = base;
    match contracts::spec(&c) {
        Ok(s) => {
            r["outcome"] = json!("spec");
            r["spec"] = s;
            r["unsupported_reason"] = Value::Null;
            r["verification"] = json!({"status":"verified","reason":null});
        }
        Err(reason) => {
            r["outcome"] = json!("unsupported");
            r["spec"] = Value::Null;
            r["unsupported_reason"] = json!(reason);
            r["verification"] = Value::Null;
        }
    }
    r
}

#[test]
fn worktable_export_writes_forge_table_and_template() {
    let (root, _store, project, branch) = fixture("export");
    let out = worktable::export(&root, &project, &branch, &Default::default()).unwrap();
    assert_eq!(out["rows"], 3);
    assert_eq!(out["started"], false);
    let table = fs::read_to_string(out["table"].as_str().unwrap()).unwrap();
    let lines: Vec<_> = table.lines().collect();
    assert_eq!(lines[0], worktable::COLUMNS.join("\t"));
    assert_eq!(lines.len(), 4);
    for (i, line) in lines[1..].iter().enumerate() {
        let cells: Vec<_> = line.split('\t').collect();
        assert_eq!(cells.len(), 12);
        assert_eq!(cells[0], format!("T{:03}", i + 1));
        assert_eq!(cells[10], worktable::OUTCOME_CELL);
        assert!(std::path::Path::new(cells[6]).is_dir());
        assert!(std::path::Path::new(cells[7]).is_file());
    }
    let template = fs::read_to_string(out["template"].as_str().unwrap()).unwrap();
    assert!(template.contains(&format!(
        "Outcome: OC-09, result line 8: {}.",
        worktable::OUTCOME_LINE
    )));
    assert!(template.contains("Posture: apply."));
    assert_eq!(template.matches("\n## ").count(), 8);
    // Nothing returned yet: no result file exists in any row.
    let folder = PathBuf::from(out["folder"].as_str().unwrap());
    assert!(!folder.join("rows/T001/result.json").exists());
    fs::write(root.join("export.json"), format!("{out:#}")).unwrap();
}

#[test]
fn worktable_join_two_returned_one_missing() {
    let (root, mut store, project, branch) = fixture("join");
    let out = worktable::export(&root, &project, &branch, &Default::default()).unwrap();
    let folder = PathBuf::from(out["folder"].as_str().unwrap());
    let run = out["run"].as_str().unwrap();
    for row in ["T001", "T003"] {
        let dir = folder.join("rows").join(row);
        fs::write(dir.join("result.json"), worker_result(&dir).to_string()).unwrap();
    }
    let join = worktable::join(&root, &project, &branch, run, false).unwrap();
    assert_eq!(join["joined"], 2);
    assert_eq!(join["missing"], 1);
    assert_eq!(join["failed"], 0);
    assert_eq!(join["specs"], 1);
    assert_eq!(join["unsupported"], 1);
    assert_eq!(join["source"], "work-table");
    assert!(join.get("model").is_none(), "no route was asked");
    let missing: Vec<_> = join["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["status"] == "missing")
        .collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["row"], "T002");
    // The join stands where the tail's own would: the latest tail revision.
    let state = contracts::tail_state(&root, &project, &branch).unwrap();
    assert_eq!(state["revision"], 2);
    assert_eq!(state["nodes"]["CONSISTENCY_JOIN"], "produced");
    assert_eq!(state["nodes"]["PHOD_SPEC"], "failed");
    assert_eq!(state["instances"][1]["spec"], "missing");
    // The readable join says it, and the work table survives the folder being rebuilt.
    let folder_path = store.open_project_folder(&project).unwrap();
    let text = fs::read_to_string(folder_path.join("product/Consistency join.md")).unwrap();
    assert!(text.contains("joined 2, missing 1, failed 0"), "{text}");
    assert!(
        folder_path
            .join("work-table")
            .join(run)
            .join("table.tsv")
            .is_file()
    );
    assert!(
        folder_path
            .join("work-table")
            .join(run)
            .join("Join revision 2.md")
            .is_file()
    );
    assert!(!out["folder"].as_str().unwrap().contains(' '));
    fs::write(root.join("join.json"), format!("{join:#}")).unwrap();
    let _ = &mut store;
}

#[test]
fn worktable_join_wrong_spec_is_failed_not_joined() {
    let (root, _store, project, branch) = fixture("failed");
    let out = worktable::export(&root, &project, &branch, &Default::default()).unwrap();
    let folder = PathBuf::from(out["folder"].as_str().unwrap());
    for row in ["T001", "T002", "T003"] {
        let dir = folder.join("rows").join(row);
        let mut r = worker_result(&dir);
        if row == "T002" {
            r["spec"]["maximum"] = json!(5);
        }
        fs::write(dir.join("result.json"), r.to_string()).unwrap();
    }
    let join = worktable::join(
        &root,
        &project,
        &branch,
        out["run"].as_str().unwrap(),
        false,
    )
    .unwrap();
    assert_eq!(
        (
            join["joined"].clone(),
            join["failed"].clone(),
            join["missing"].clone()
        ),
        (json!(2), json!(1), json!(0))
    );
    assert!(
        join["content"]
            .as_str()
            .unwrap()
            .contains("Speaker note (T002): failed")
    );
}

#[test]
fn worktable_bridge_commands_and_refusals() {
    let (root, _store, project, branch) = fixture("bridge");
    let args = json!({"project_id":project,"branch_id":branch,"account":"fixture-account"});
    let out = worktable::command(&root, "qroc-worktable-export", &args).unwrap();
    let table = fs::read_to_string(out["table"].as_str().unwrap()).unwrap();
    assert!(table.lines().nth(1).unwrap().ends_with("\tfixture-account"));
    let join = worktable::command(
        &root,
        "qroc-worktable-join",
        &json!({"project_id":project,"branch_id":branch,"run":out["run"]}),
    )
    .unwrap();
    assert_eq!(join["missing"], 3);
    assert!(worktable::command(&root, "qroc-worktable-start", &args).is_err());
    let bad = json!({"project_id":project,"branch_id":branch,"effort":"high"});
    assert!(worktable::command(&root, "qroc-worktable-export", &bad).is_err());
    // A branch without contracts has nothing to hand over.
    let empty = json!({"project_id":project,"branch_id":"nobranch"});
    assert!(worktable::command(&root, "qroc-worktable-export", &empty).is_err());
}
