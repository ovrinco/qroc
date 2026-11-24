#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{bridge::Bridge, qroc::QrocStore};
use serde_json::json;
use std::{fs, path::PathBuf};

fn fixture_root(label: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(format!("fixtures-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}
#[test]
fn fixture_revisions_remain_in_step() {
    let root = fixture_root("folders");
    let mut store = QrocStore::open(&root).unwrap();
    let p = store
        .command(
            "qroc-project-create",
            &json!({"title":"Fixture project", "brain_dump":"Fixture idea only"}),
        )
        .unwrap();
    let id = p["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    let b = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":id,"name":"Fixture branch"}),
        )
        .unwrap();
    let branch = b["id"].as_str().unwrap();
    for kind in [
        "STRATEGY_BRIEF",
        "BUSINESS_PLAN",
        "MESSAGING_FRAMEWORK",
        "TERMS",
    ] {
        store.command("qroc-document-save", &json!({"project_id":id,"branch_id":branch,"document_type":kind,"content":format!("Fixture {kind} revision 1")})).unwrap();
    }
    let folder = store.open_project_folder(id).unwrap();
    assert_eq!(
        fs::read_to_string(folder.join("product/Strategy Brief.md")).unwrap(),
        "Fixture STRATEGY_BRIEF revision 1"
    );
    let listing = store
        .command(
            "qroc-document-list",
            &json!({"project_id":id,"branch_id":branch}),
        )
        .unwrap();
    fs::write(
        root.join("before-records.json"),
        serde_json::to_vec_pretty(&listing).unwrap(),
    )
    .unwrap();
    let list = std::process::Command::new("find")
        .arg(&folder)
        .args(["-type", "f"])
        .output()
        .unwrap();
    fs::write(root.join("before-listing.txt"), list.stdout).unwrap();
    store.command("qroc-document-save", &json!({"project_id":id,"branch_id":branch,"document_type":"STRATEGY_BRIEF","content":"Fixture revision 2","source_revision":1})).unwrap();
    assert_eq!(
        fs::read_to_string(folder.join("product/Strategy Brief.md")).unwrap(),
        "Fixture revision 2"
    );
    assert_eq!(
        fs::read_to_string(folder.join("product/revisions/Strategy Brief - revision 1.md"))
            .unwrap(),
        "Fixture STRATEGY_BRIEF revision 1"
    );
    let history = store
        .command(
            "qroc-document-history",
            &json!({"project_id":id,"branch_id":branch,"document_type":"STRATEGY_BRIEF"}),
        )
        .unwrap();
    fs::write(
        root.join("after-history.json"),
        serde_json::to_vec_pretty(&history).unwrap(),
    )
    .unwrap();
    for (domain, title) in [
        ("business", "Business Plan"),
        ("presence", "Messaging Framework"),
        ("legal", "Terms"),
    ] {
        assert!(folder.join(domain).join(format!("{title}.md")).is_file());
    }
    for domain in [
        "idea",
        "product",
        "business",
        "presence",
        "legal",
        "work-table",
        "media",
    ] {
        assert!(folder.join(domain).is_dir());
    }
    let mut reopened = QrocStore::open(&root).unwrap();
    let result = reopened
        .command("qroc-project-open-folder", &json!({"project_id":id}))
        .unwrap();
    assert_eq!(result["folder"].as_str().unwrap(), folder.to_str().unwrap());
    assert!(reopened.open_project_folder("missing").is_err());
    println!("FIXTURE_ROOT={}", root.display());
}
#[test]
fn bridge_returns_file_manager_location_and_rejects_unknown_owner() {
    let root = fixture_root("bridge");
    let mut bridge = Bridge::open(&root).unwrap();
    let p = bridge.reply(json!({"version":1,"id":"create","command":"qroc-project-create","args":{"title":"Fixture bridge project","brain_dump":"Fixture only"}}));
    assert_eq!(p["ok"], true);
    let r = bridge.reply(json!({"version":1,"id":"open","command":"qroc-project-open-folder","args":{"project_id":p["result"]["id"]}}));
    assert_eq!(r["ok"], true);
    assert!(PathBuf::from(r["result"]["folder"].as_str().unwrap()).is_dir());
    let r = bridge.reply(json!({"version":1,"id":"bad","command":"qroc-project-open-folder","args":{"project_id":"../../outside"}}));
    assert_eq!(r["ok"], false);
}
#[test]
fn branches_are_separate_and_duplicate_names_are_rejected() {
    let root = fixture_root("branches");
    let mut store = QrocStore::open(&root).unwrap();
    let p = store
        .command(
            "qroc-project-create",
            &json!({"title":"Fixture branches","brain_dump":"Fixture only"}),
        )
        .unwrap();
    let id = p["id"].as_str().unwrap();
    hardening::pass_hardening(&root);
    for branch_name in ["First", "Second"] {
        let b = store
            .command(
                "qroc-branch-create",
                &json!({"project_id":id,"name":branch_name}),
            )
            .unwrap();
        store.command("qroc-document-save", &json!({"project_id":id,"branch_id":b["id"],"document_type":"PRD","content":format!("Fixture {branch_name}")})).unwrap();
    }
    let folder = store.open_project_folder(id).unwrap();
    assert!(!folder.join("product/PRD.md").exists());
    for b in ["First", "Second"] {
        assert_eq!(
            fs::read_to_string(folder.join(format!("product/PRD ({b}).md"))).unwrap(),
            format!("Fixture {b}")
        );
    }
    let b = store
        .command(
            "qroc-branch-create",
            &json!({"project_id":id,"name":"First"}),
        )
        .unwrap();
    assert!(store.command("qroc-document-save", &json!({"project_id":id,"branch_id":b["id"],"document_type":"PRD","content":"Fixture duplicate"})).is_err());
    assert_eq!(
        fs::read_to_string(folder.join("product/PRD (First).md")).unwrap(),
        "Fixture First"
    );
}
#[test]
fn symlinks_and_project_name_collisions_do_not_overwrite() {
    use std::os::unix::fs::symlink;
    let root = fixture_root("safety");
    let mut store = QrocStore::open(&root).unwrap();
    let args = json!({"title":"Fixture same name","brain_dump":"Fixture only"});
    let p = store.command("qroc-project-create", &args).unwrap();
    let folder = store
        .open_project_folder(p["id"].as_str().unwrap())
        .unwrap();
    let p2 = store.command("qroc-project-create", &args).unwrap();
    assert!(
        store
            .open_project_folder(p2["id"].as_str().unwrap())
            .is_err()
    );
    let other = root.join("outside");
    fs::create_dir(&other).unwrap();
    fs::rename(&folder, root.join("saved-projection")).unwrap();
    symlink(&other, &folder).unwrap();
    assert!(
        store
            .open_project_folder(p["id"].as_str().unwrap())
            .is_err()
    );
    assert_eq!(fs::read_dir(&other).unwrap().count(), 0);
    let p3 = store
        .command(
            "qroc-project-create",
            &json!({"title":"../escape","brain_dump":"Fixture only"}),
        )
        .unwrap();
    assert!(
        store
            .open_project_folder(p3["id"].as_str().unwrap())
            .is_err()
    );
}
