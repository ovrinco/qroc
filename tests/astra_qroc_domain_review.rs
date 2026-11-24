#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::qroc::QrocStore;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
fn setup() -> (QrocStore, PathBuf, Value, Value) {
    let root =
        PathBuf::from(std::env::var("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"))
            .join(format!(
                "qroc-reviewed-transition-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
    let mut s = QrocStore::open(&root).unwrap();
    let p=s.command("qroc-project-create",&json!({"title":"Gate review contract fixture","brain_dump":"Deterministic regression only, not a real-project approval"})).unwrap();
    hardening::pass_hardening(&root);
    let b = s
        .command(
            "qroc-branch-create",
            &json!({"project_id":p["id"],"name":"Reviewed","model":"no-model"}),
        )
        .unwrap();
    (s, root, p, b)
}
fn args(p: &Value, b: &Value, t: &str, r: u32) -> Value {
    json!({"project_id":p["id"],"branch_id":b["id"],"document_type":t,"revision":r})
}
/// Rows T028-050 and T661-250: passed gate reviews alone never produce Product, so no
/// later domain can be triggered and the project stays in Product.
fn assert_product_fence(s: &mut QrocStore, p: &Value, a: &Value) {
    let refused = s.command("qroc-domain-advance", a).unwrap_err().to_string();
    assert!(
        refused.contains("Complete the Product pipeline before triggering a later domain"),
        "{refused}"
    );
    let detail = s
        .command("qroc-project-detail", &json!({"id":p["id"]}))
        .unwrap();
    assert_eq!(detail["project"]["current_domain"], "PRODUCT");
    assert!(
        detail["domains"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["status"] == "predicted"),
        "{}",
        detail["domains"]
    );
}
/// A completed pipeline receipt fixture (as in tests/domains.rs); the native journey
/// supplies real receipts. Past it, the review assertions below are tested as written.
fn complete_product(root: &std::path::Path, p: &Value) {
    let pipeline = root.join("pipeline").join(p["id"].as_str().unwrap());
    std::fs::create_dir_all(&pipeline).unwrap();
    std::fs::write(
        pipeline.join("Pipeline run.json"),
        json!({"project_id":p["id"],"status":"completed","steps":qroc_standalone::pipeline::STEPS.iter().map(|s| json!({"step":s.0,"status":"completed"})).collect::<Vec<_>>()}).to_string(),
    )
    .unwrap();
}
/// A gate verdict over the rubric's own criteria (names and maxima), summing exactly to `score`.
fn verdict(t: &str, score: u32) -> String {
    let rubric = qroc_standalone::gate_rubric::rubric(t).unwrap().unwrap();
    let mut scores = rubric
        .criteria
        .iter()
        .map(|c| c.max * score / 100)
        .collect::<Vec<_>>();
    let mut left = score - scores.iter().sum::<u32>();
    for (value, c) in scores.iter_mut().zip(&rubric.criteria) {
        let add = left.min(c.max - *value);
        *value += add;
        left -= add;
    }
    let mut text = format!("Regression fixture only.\n[SCORE]: {score}\n");
    for (c, value) in rubric.criteria.iter().zip(scores) {
        text.push_str(&format!(
            "- {}: {value}/{} - deterministic test input\n",
            c.name, c.max
        ));
    }
    text
}
fn save(s: &mut QrocStore, p: &Value, b: &Value, t: &str, r: u32, score: u32) -> Value {
    s.command("qroc-document-save",&json!({"project_id":p["id"],"branch_id":b["id"],"document_type":t,"source_revision":r,"content":verdict(t, score)})).unwrap()
}
#[test]
fn reviewed_current_gates_advance_atomically_and_survive_restart() {
    let (mut s, root, p, b) = setup();
    for t in ["GATE_1", "GATE_2", "GATE_3"] {
        let d = save(&mut s, &p, &b, t, 0, 95);
        let mut a = args(&p, &b, t, 1);
        a["sha256"] = d["sha256"].clone();
        a["decision"] = json!("passed");
        s.command("qroc-gate-review", &a)
            .expect("Soliton must expose retained owner gate review");
    }
    let mut a = args(&p, &b, "GATE_3", 1);
    a["target_domain"] = json!("BUSINESS");
    a["expected_domain"] = json!("PRODUCT");
    assert_product_fence(&mut s, &p, &a);
    complete_product(&root, &p);
    let moved = s.command("qroc-domain-advance", &a).unwrap();
    assert_eq!(moved["current_domain"], "BUSINESS");
    assert_eq!(
        moved["domain_transitions"][0]["gates"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(s.command("qroc-domain-advance", &a).is_err());
    drop(s);
    let mut s = QrocStore::open(&root).unwrap();
    assert_eq!(
        s.command("qroc-project-detail", &json!({"id":p["id"]}))
            .unwrap()["project"]["current_domain"],
        "BUSINESS"
    );
}
#[test]
fn failure_stale_revision_and_unreviewed_gate_never_advance() {
    let (mut s, _, p, b) = setup();
    let d = save(&mut s, &p, &b, "GATE_3", 0, 78);
    let mut a = args(&p, &b, "GATE_3", 1);
    a["sha256"] = d["sha256"].clone();
    a["decision"] = json!("passed");
    assert!(s.command("qroc-gate-review", &a).is_err());
    a["decision"] = json!("failed");
    s.command("qroc-gate-review", &a)
        .expect("Record a failed gate without advancing");
    save(&mut s, &p, &b, "GATE_3", 1, 95);
    assert!(s.command("qroc-gate-review", &a).is_err());
    a["revision"] = json!(2);
    a["target_domain"] = json!("BUSINESS");
    a["expected_domain"] = json!("PRODUCT");
    assert!(s.command("qroc-domain-advance", &a).is_err());
    assert_eq!(
        s.command("qroc-project-detail", &json!({"id":p["id"]}))
            .unwrap()["project"]["current_domain"],
        "PRODUCT"
    );
}

#[test]
fn new_gate_revision_and_mixed_branch_reviews_cannot_reuse_prior_approval() {
    let (mut s, root, p, b) = setup();
    for t in ["GATE_1", "GATE_2", "GATE_3"] {
        let d = save(&mut s, &p, &b, t, 0, 95);
        let mut a = args(&p, &b, t, 1);
        a["sha256"] = d["sha256"].clone();
        a["decision"] = json!("passed");
        s.command("qroc-gate-review", &a).unwrap();
    }
    save(&mut s, &p, &b, "GATE_1", 1, 95);
    let mut a = args(&p, &b, "GATE_3", 1);
    a["target_domain"] = json!("BUSINESS");
    a["expected_domain"] = json!("PRODUCT");
    assert_product_fence(&mut s, &p, &a);
    complete_product(&root, &p);
    assert!(
        s.command("qroc-domain-advance", &a)
            .unwrap_err()
            .to_string()
            .contains("passing review")
    );
    let other = s
        .command(
            "qroc-branch-create",
            &json!({"project_id":p["id"],"name":"Other branch","model":"no-model"}),
        )
        .unwrap();
    let d = save(&mut s, &p, &other, "GATE_3", 0, 95);
    let mut review = args(&p, &other, "GATE_3", 1);
    review["sha256"] = d["sha256"].clone();
    review["decision"] = json!("passed");
    s.command("qroc-gate-review", &review).unwrap();
    a["branch_id"] = other["id"].clone();
    assert!(s.command("qroc-domain-advance", &a).is_err());
    assert_eq!(
        s.command("qroc-project-detail", &json!({"id":p["id"]}))
            .unwrap()["project"]["current_domain"],
        "PRODUCT"
    );
}
