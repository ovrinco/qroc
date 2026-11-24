#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{Error, qroc::QrocStore};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn workspace() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("soliton-qroc-contract-{suffix}"));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn projects_branches_and_revisions_survive_restart() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store.command("qroc-project-create", &json!({"title":"North star","brain_dump":"A local product with explicit constraints."})).unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"baseline","model":"qwen3.8-27b-desktop"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap().to_owned();
    let first = store.command("qroc-document-save", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"revision one"})).unwrap();
    let second = store.command("qroc-document-save", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"revision two","source_revision":first["revision"]})).unwrap();
    assert_eq!(first["revision"], 1);
    assert_eq!(second["revision"], 2);
    drop(store);
    let mut reopened = QrocStore::open(&root).unwrap();
    let history = reopened
        .command(
            "qroc-document-history",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
        )
        .unwrap();
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gate_evaluation_requires_explicit_score_and_criteria() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Gate test","brain_dump":"Test material"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"review"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap();
    let doc = store.command("qroc-document-save", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"GATE_1","content":"[SCORE]: 94\n- Completeness: 6/10 - two sections need evidence\n- Feasibility: 10/10 - supported"})).unwrap();
    let result = store.command("qroc-gate-evaluate", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"GATE_1","revision":doc["revision"]})).unwrap();
    assert_eq!(result["status"], "failed");
    assert_eq!(result["criteria"]["completeness"]["score"], 6);
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("Completeness scored 6 of 10, below its floor of 7.")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn gate_score_without_criteria_is_not_reported_as_passed() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Gate no rubric","brain_dump":"Test material"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"review"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap();
    let doc = store
        .command(
            "qroc-document-save",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"GATE_1","content":"[SCORE]: 95\nNo supplied rubric."}),
        )
        .unwrap();
    let result = store
        .command(
            "qroc-gate-evaluate",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"GATE_1","revision":doc["revision"]}),
        )
        .unwrap();
    assert_eq!(result["status"], "failed");
    assert!(result["reason"].as_str().unwrap().contains("criterion"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn document_and_source_paths_are_constrained() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Path test","brain_dump":"Test material"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"review"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap();
    let error = store.command("qroc-document-save", &json!({"project_id":"../escape","branch_id":branch_id,"document_type":"PRD","content":"unsafe"})).unwrap_err();
    assert!(matches!(error, Error::Invalid(_)));
    let error = store
        .command(
            "qroc-project-save",
            &json!({"id":project_id,"sources":[{"id":"x","name":"x","path":"relative.txt"}]}),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Invalid(_)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn knowledge_base_items_are_explicitly_selected_and_durable() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"KB test","brain_dump":"Project context"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    hardening::pass_hardening(&root);
    let item = store.command("qroc-kb-save", &json!({"project_id":project_id,"title":"Customer interview","kind":"note","content":"Users need a local workflow.","selected":true})).unwrap();
    let context = store
        .command("qroc-kb-context", &json!({"project_id":project_id}))
        .unwrap();
    assert!(context["context"].as_str().unwrap().contains("Users need"));
    store.command("qroc-kb-save", &json!({"project_id":project_id,"id":item["id"],"title":"Customer interview","kind":"note","content":"Users need a local workflow.","selected":false})).unwrap();
    let context = store
        .command("qroc-kb-context", &json!({"project_id":project_id}))
        .unwrap();
    assert_eq!(context["items"].as_array().unwrap().len(), 0);
    drop(store);
    let mut reopened = QrocStore::open(&root).unwrap();
    let listed = reopened
        .command("qroc-kb-list", &json!({"project_id":project_id}))
        .unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn drafts_round_trip_by_project_branch_and_document_without_revision_overwrite() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Draft test","brain_dump":"Project context"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"working"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap().to_owned();
    let missing = store
        .command(
            "qroc-draft-get",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
        )
        .unwrap();
    assert_eq!(missing, json!({}));
    let saved = store
        .command(
            "qroc-draft-save",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"autosaved draft","base_revision":3}),
        )
        .unwrap();
    assert_eq!(saved["content"], "autosaved draft");
    assert_eq!(saved["base_revision"], 3);
    drop(store);
    let mut reopened = QrocStore::open(&root).unwrap();
    let loaded = reopened
        .command(
            "qroc-draft-get",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
        )
        .unwrap();
    assert_eq!(loaded["content"], "autosaved draft");
    assert_eq!(loaded["base_revision"], 3);
    reopened
        .command(
            "qroc-draft-clear",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
        )
        .unwrap();
    assert_eq!(
        reopened
            .command(
                "qroc-draft-get",
                &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
            )
            .unwrap(),
        json!({})
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn import_preserves_real_exported_material_as_owned_sources_and_revisions() {
    let root = workspace();
    let source_root = workspace();
    fs::create_dir_all(source_root.join("product")).unwrap();
    fs::write(
        source_root.join("product/01_STRATEGY_BRIEF.md"),
        "# Strategy\nEvidence and open questions.",
    )
    .unwrap();
    fs::write(
        source_root.join("product/02_PRD.md"),
        "# Requirements\nMUST preserve provenance.",
    )
    .unwrap();
    fs::write(source_root.join("product/README.txt"), "Unmapped notes").unwrap();
    let mut store = QrocStore::open(&root).unwrap();
    let imported = store
        .command(
            "qroc-project-import",
            &json!({"path":source_root,"title":"Imported cascade"}),
        )
        .unwrap();
    assert_eq!(imported["imported"], 2);
    assert_eq!(imported["source_count"], 3);
    let project_id = imported["project"]["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let branch_id = imported["branch"]["id"].as_str().unwrap();
    let listed = store
        .command(
            "qroc-document-list",
            &json!({"project_id":project_id,"branch_id":branch_id}),
        )
        .unwrap();
    assert_eq!(listed["documents"].as_array().unwrap().len(), 2);
    let project_detail = store
        .command("qroc-project-detail", &json!({"id":project_id}))
        .unwrap();
    let copied_path = project_detail["project"]["sources"][0]["path"]
        .as_str()
        .unwrap();
    assert!(copied_path.starts_with(root.to_str().unwrap()));
    assert!(PathBuf::from(copied_path).is_file());
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn skills_import_is_selected_knowledge_and_never_execution() {
    let root = workspace();
    let source_root = workspace();
    fs::create_dir_all(source_root.join("skill")).unwrap();
    fs::write(
        source_root.join("skill/SKILL.md"),
        "Review skill: explicit evidence.",
    )
    .unwrap();
    fs::write(source_root.join("skill/notes.md"), "Supporting notes.").unwrap();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Skill test","brain_dump":"Local authoring"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let result = store
        .command(
            "qroc-skills-import",
            &json!({"project_id":project_id,"path":source_root}),
        )
        .unwrap();
    assert_eq!(result["executed"], false);
    assert_eq!(result["imported"].as_array().unwrap().len(), 2);
    let context = store
        .command("qroc-kb-context", &json!({"project_id":project_id}))
        .unwrap();
    assert!(
        context["context"]
            .as_str()
            .unwrap()
            .contains("Review skill")
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(source_root);
}

#[test]
fn generation_context_failure_does_not_leave_orphan_running_job() {
    let root = workspace();
    let attachment = root.join("attachment.md");
    fs::write(&attachment, "original source").unwrap();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Generation safety","brain_dump":"Bounded generation","sources":[{"id":"src","name":"source","path":attachment}]}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap().to_owned();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"main"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap();
    fs::write(&attachment, "changed after pin").unwrap();
    let error = store
        .command(
            "qroc-generate-start",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
        )
        .unwrap_err();
    assert!(matches!(error, Error::Invalid(_)));
    assert_eq!(
        fs::read_dir(root.join("jobs")).unwrap().count(),
        0,
        "context validation must precede durable job creation"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corrupt_durable_project_record_is_reported_on_restart() {
    let root = workspace();
    fs::create_dir_all(root.join("projects")).unwrap();
    fs::write(root.join("projects/bad.json"), b"{not-json").unwrap();
    let result = QrocStore::open(&root);
    assert!(matches!(result, Err(Error::Invalid(_))));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn restart_reconciles_published_generation_before_receipt() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Receipt recovery","brain_dump":"Durable output"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"main"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap();
    let document = store
        .command(
            "qroc-document-save",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"published generation"}),
        )
        .unwrap();
    let job_id = "job-reconcile-1";
    fs::write(
        root.join("jobs").join(format!("{job_id}.json")),
        serde_json::to_vec(&json!({
            "id":job_id,
            "project_id":project_id,
            "branch_id":branch_id,
            "document_type":"PRD",
            "state":"running",
            "model":"qwen3.8-27b-desktop",
            "error":null,
            "document_id":null,
            "output_sha256":document["sha256"],
            "started_at":1,
            "finished_at":null
        }))
        .unwrap(),
    )
    .unwrap();
    drop(store);
    let mut reopened = QrocStore::open(&root).unwrap();
    let status = reopened
        .command("qroc-job-status", &json!({"id":job_id}))
        .unwrap();
    assert_eq!(status["state"], "succeeded");
    assert_eq!(status["document_id"], document["id"]);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn document_save_rejects_stale_head_without_creating_a_revision() {
    let root = workspace();
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"CAS document","brain_dump":"Concurrent edits"}),
        )
        .unwrap();
    let project_id = project["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let branch = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":project_id,"name":"main"}),
        )
        .unwrap();
    let branch_id = branch["id"].as_str().unwrap();
    let first = store
        .command(
            "qroc-document-save",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"first","source_revision":0}),
        )
        .unwrap();
    let second = store
        .command(
            "qroc-document-save",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"second","source_revision":first["revision"]}),
        )
        .unwrap();
    let stale = store.command(
        "qroc-document-save",
        &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD","content":"stale","source_revision":first["revision"]}),
    );
    assert!(stale.is_err());
    let history = store
        .command(
            "qroc-document-history",
            &json!({"project_id":project_id,"branch_id":branch_id,"document_type":"PRD"}),
        )
        .unwrap();
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    assert_eq!(history["items"][1]["id"], second["id"]);
    let _ = fs::remove_dir_all(root);
}
