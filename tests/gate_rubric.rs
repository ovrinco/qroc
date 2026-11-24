#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{gate_rubric, qroc::QrocStore};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn workspace(tag: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::var_os("QROC_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = base.join(format!("qroc-gate-rubric-{tag}-{suffix}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn evaluate(tag: &str, kind: &str, content: &str) -> Value {
    let root = workspace(tag);
    let mut store = QrocStore::open(&root).unwrap();
    let project = store
        .command(
            "qroc-project-create",
            &json!({"title":"Gate rubric","brain_dump":"Test material"}),
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
    let doc = store.command("qroc-document-save", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":kind,"content":content})).unwrap();
    let result = store.command("qroc-gate-evaluate", &json!({"project_id":project_id,"branch_id":branch_id,"document_type":kind,"revision":doc["revision"]})).unwrap();
    let _ = fs::remove_dir_all(root);
    result
}

/// A reply in the form the gate prompt asks for, with the shipped Gate 1 criteria.
fn gate1_reply(scores: [u32; 6]) -> String {
    let names = [
        ("Problem and Users", 15),
        ("Requirement Testability", 25),
        ("Risk Coverage", 15),
        ("Validation Readiness", 20),
        ("Architecture Fit", 15),
        ("Cross-Document Consistency", 10),
    ];
    let total: u32 = scores.iter().sum();
    let mut text = format!("# Gate 1\n\n[SCORE]: {total}\n");
    for ((name, max), score) in names.iter().zip(scores) {
        text.push_str(&format!(
            "- {name}: {score}/{max} - checked against the supplied documents\n"
        ));
    }
    text.push_str("\n## Decision\n1. Recorded above.\n");
    text
}

#[test]
fn well_formed_gate_reply_is_judged_against_the_threshold() {
    let passed = evaluate("pass", "GATE_1", &gate1_reply([14, 23, 14, 18, 14, 9]));
    assert_eq!(passed["status"], "passed", "{passed}");
    assert_eq!(passed["score"], 92);
    assert_eq!(passed["threshold"], 90);
    assert_eq!(passed["criteria"]["requirement testability"]["max"], 25);
    // Same form, honest weak plan: total 85 is under 90 and must fail.
    let weak = evaluate("weak", "GATE_1", &gate1_reply([13, 20, 13, 17, 13, 9]));
    assert_eq!(weak["score"], 85);
    assert_eq!(weak["status"], "failed", "{weak}");
}

#[test]
fn gate_that_declines_to_score_is_reported_in_words_as_failed() {
    let declined = evaluate(
        "declined",
        "GATE_1",
        "# GATE_1\n\nNo numerical score, pass threshold, or approval authority is supplied. None is inferred.\n\n| C1 | Problem | Not assigned / Not defined | Documented |\n",
    );
    assert_eq!(declined["status"], "failed", "{declined}");
    assert!(declined["score"].is_null());
    assert!(
        declined["reason"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("the gate gave no score"),
        "{declined}"
    );
    // A criterion-shaped line without feedback cannot be read: failed, in words, not a crash.
    let unreadable = evaluate(
        "unreadable",
        "GATE_1",
        "Draft review.\n- Scope: schema/timing unresolved\n",
    );
    assert_eq!(unreadable["status"], "failed", "{unreadable}");
    let reason = unreadable["reason"].as_str().unwrap().to_ascii_lowercase();
    assert!(reason.contains("the gate gave no score"), "{reason}");
}

#[test]
fn replaced_rubric_file_changes_the_gate_without_code_change() {
    let dir = workspace("file");
    let file = dir.join("rubrics.json");
    fs::write(
        &file,
        json!({"schema":"qroc-gate-rubrics/1","standing":"drafted","gates":{"GATE_1":{
            "meaning":"Replaced meaning for Gate 1.","joins":["PRD"],"criteria":[
            {"name":"Alpha","max":40,"question":"Alpha?"},
            {"name":"Beta","max":30,"question":"Beta?"},
            {"name":"Gamma","max":20,"question":"Gamma?"},
            {"name":"Delta","max":10,"question":"Delta?"}]}}})
        .to_string(),
    )
    .unwrap();
    // SAFETY: the only test in this binary that reads or writes this variable.
    unsafe { std::env::set_var(gate_rubric::FILE_VARIABLE, &file) };
    let rubric = gate_rubric::rubric("GATE_1")
        .unwrap()
        .expect("replaced gate");
    let prompt = gate_rubric::prompt("GATE_1", &rubric).unwrap();
    unsafe { std::env::remove_var(gate_rubric::FILE_VARIABLE) };
    assert!(prompt.contains("Replaced meaning for Gate 1."));
    assert!(prompt.contains("- Alpha: <score>/40 - <feedback>"));
    assert!(!prompt.contains("Requirement Testability"));
    // The bar is the engine's, not the file's.
    assert!(prompt.contains("at least 90 of 100"));
    // A file that would break the 100-point rubric is refused.
    fs::write(&file, r#"{"schema":"qroc-gate-rubrics/1","gates":{"GATE_1":{"meaning":"m","criteria":[{"name":"A","max":10,"question":"q"},{"name":"B","max":10,"question":"q"},{"name":"C","max":10,"question":"q"},{"name":"D","max":10,"question":"q"}]}}}"#).unwrap();
    assert!(gate_rubric::load_from(&file).is_err());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn shipped_runtime_rubrics_are_the_source_rubrics() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read(root.join("src/gates/rubrics.json")).unwrap();
    let shipped = fs::read(root.join("runtime/gates/rubrics.json")).unwrap();
    assert_eq!(
        source, shipped,
        "runtime/gates/rubrics.json must equal src/gates/rubrics.json"
    );
}

#[test]
fn live_gate1_reply_is_read_from_its_score_block_only() {
    // The live Gate 1 reply of T404W3, byte for byte: a score block, then prose with a
    // bold summary "**Gate 1 FAILS: 81/100.**" that is not a criterion.
    let content = include_str!("fixtures/t404w3_gate1.md");
    let result = evaluate("live-gate1", "GATE_1", content);
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    assert_eq!(result["status"], "failed", "{result}");
    assert_eq!(result["score"], 81, "{result}");
    assert_eq!(result["threshold"], 90);
    assert_eq!(
        result["reason"], "Score 81 of 100 is below the bar of 90.",
        "{result}"
    );
    let criteria = result["criteria"].as_object().unwrap();
    assert_eq!(criteria.len(), 6, "{result}");
    let sum: u64 = criteria
        .values()
        .map(|c| c["score"].as_u64().unwrap())
        .sum();
    assert_eq!(sum, 81);
    assert!(
        criteria
            .values()
            .all(|c| !c["feedback"].as_str().unwrap().is_empty())
    );
    assert_eq!(criteria["risk coverage"]["score"], 14);
    assert!(
        criteria["cross-document consistency"]["feedback"]
            .as_str()
            .unwrap()
            .starts_with("The documents agree")
    );
}

#[test]
fn passing_reply_with_summary_prose_still_passes() {
    let mut content = gate1_reply([14, 23, 14, 18, 14, 9]);
    content.push_str("\n**Gate 1 PASSES: 92/100.**\n\n- Note: 3/4 of the gaps are closed\n");
    let result = evaluate("pass-prose", "GATE_1", &content);
    assert_eq!(result["status"], "passed", "{result}");
    assert_eq!(result["score"], 92);
    assert_eq!(
        result["reason"],
        "Score 92 of 100 meets the bar of 90, and every criterion meets its floor."
    );
    assert_eq!(result["criteria"].as_object().unwrap().len(), 6);
}

#[test]
fn reply_without_score_block_fails_in_words() {
    let result = evaluate(
        "no-block",
        "GATE_1",
        "## Decision\n\n**Gate 1 FAILS: 81/100.**\n\n- Risk Coverage: 14/15 - fine\n",
    );
    assert_eq!(result["status"], "failed", "{result}");
    assert!(result["score"].is_null());
    assert!(result["criteria"].as_object().unwrap().is_empty());
    let reason = result["reason"].as_str().unwrap().to_ascii_lowercase();
    assert!(reason.contains("the gate gave no score"), "{reason}");
}

#[test]
fn criterion_below_its_floor_is_named_in_the_reason() {
    // Total 91 clears the bar; Requirement Testability 17/25 is under its 70% floor of 18.
    let result = evaluate("floor", "GATE_1", &gate1_reply([15, 17, 15, 20, 15, 9]));
    assert_eq!(result["score"], 91);
    assert_eq!(result["status"], "failed", "{result}");
    assert_eq!(
        result["reason"], "Requirement testability scored 17 of 25, below its floor of 18.",
        "{result}"
    );
}
