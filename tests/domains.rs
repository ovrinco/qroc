#[path = "common/hardening.rs"]
mod hardening;
use qroc_standalone::{Result, qroc::QrocStore};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const ORDER: [&str; 7] = [
    "PRODUCT",
    "DISTRIBUTION",
    "ECOSYSTEM",
    "BRAND",
    "DATA",
    "COMMUNITY",
    "CAPITAL",
];

fn root(label: &str) -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after epoch")
        .as_nanos();
    let root =
        PathBuf::from(std::env::var_os("QROC_TEST_ROOT").expect("explicit private QROC_TEST_ROOT"))
            .join("domain-tests")
            .join(format!("{label}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root)?;
    Ok(root)
}

fn ids(detail: &Value) -> Vec<&str> {
    detail["domains"]
        .as_array()
        .expect("domain overview array")
        .iter()
        .map(|d| d["id"].as_str().expect("domain id"))
        .collect()
}

#[test]
fn new_project_lists_all_domains_predicted_until_work_is_produced() -> Result<()> {
    let mut store = QrocStore::open(&root("new")?)?;
    let p = store.command(
        "qroc-project-create",
        &json!({"title":"Seven domains", "brain_dump":"Test fixture only"}),
    )?;
    let d = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    assert_eq!(ids(&d), ORDER);
    for domain in d["domains"].as_array().expect("overview").iter() {
        assert_eq!(domain["status"], "predicted");
        assert!(
            !domain["purpose"]
                .as_str()
                .expect("plain purpose")
                .trim()
                .is_empty()
        );
    }
    println!("NEW_PROJECT_DOMAINS={}", d["domains"]);
    Ok(())
}

#[test]
fn advance_produces_predicted_distribution_without_reordering_and_survives_restart() -> Result<()> {
    let root = root("advance")?;
    let mut store = QrocStore::open(&root)?;
    let p = store.command(
        "qroc-project-create",
        &json!({"title":"Advance domains", "brain_dump":"Gate fixture only"}),
    )?;
    hardening::pass_hardening(&root);
    let b = store.command(
        "qroc-branch-create",
        &json!({"project_id":p["id"],"name":"Reviewed"}),
    )?;
    let mut a = json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"GATE_3","revision":1,"expected_domain":"PRODUCT","target_domain":"DISTRIBUTION"});
    for kind in ["GATE_1", "GATE_2", "GATE_3"] {
        let rubric = qroc_standalone::gate_rubric::rubric(kind)?.expect("native gate rubric");
        let mut content = "Test fixture only.\n[SCORE]: 100\n".to_owned();
        for c in rubric.criteria {
            content.push_str(&format!(
                "- {}: {}/{} - test evidence\n",
                c.name, c.max, c.max
            ));
        }
        let d = store.command("qroc-document-save", &json!({"project_id":p["id"],"branch_id":b["id"],"document_type":kind,"source_revision":0,"content":content}))?;
        let review = json!({"project_id":p["id"],"branch_id":b["id"],"document_type":kind,"revision":d["revision"],"sha256":d["sha256"],"decision":"passed"});
        if kind == "GATE_3" {
            assert!(
                store.command("qroc-domain-advance", &a).is_err(),
                "unreviewed final gate must block"
            );
        }
        store.command("qroc-gate-review", &review)?;
    }
    let early = store
        .command(
            "qroc-generate-start",
            &json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"MARKET_ANALYSIS"}),
        )
        .expect_err("later generation before Product must refuse");
    assert!(early.to_string().contains("Complete the Product pipeline"));
    let before = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    assert_eq!(ids(&before), ORDER);
    assert_eq!(before["domains"][1]["status"], "predicted");
    // Gate documents of a Product that is not produced are drafts, not produced items.
    assert_eq!(before["domains"][0]["status"], "predicted");
    assert_eq!(before["domains"][0]["produced_items"], json!([]));
    assert_eq!(
        before["domains"][0]["drafted_items"],
        json!(["GATE_1", "GATE_2", "GATE_3"])
    );
    assert!(
        store.command("qroc-domain-advance", &a).is_err(),
        "passed gates alone do not produce Product"
    );
    // A completed pipeline receipt fixture; the native journey supplies real receipts.
    let pipeline = root
        .join("pipeline")
        .join(p["id"].as_str().expect("project"));
    fs::create_dir_all(&pipeline)?;
    fs::write(
        pipeline.join("Pipeline run.json"),
        serde_json::to_vec(
            &json!({"project_id":p["id"],"status":"completed","steps":qroc_standalone::pipeline::STEPS.iter().map(|s| json!({"step":s.0,"status":"completed"})).collect::<Vec<_>>()}),
        )?,
    )?;
    let product = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    assert_eq!(product["domains"][0]["status"], "produced");
    assert_eq!(
        product["domains"][0]["produced_items"],
        json!(["GATE_1", "GATE_2", "GATE_3"])
    );
    assert_eq!(product["domains"][0]["drafted_items"], json!([]));
    let advanced = store.command("qroc-domain-advance", &a)?;
    assert_eq!(advanced["current_domain"], "DISTRIBUTION");
    assert_eq!(
        advanced["domain_transitions"][0]["gates"]
            .as_array()
            .expect("pins")
            .len(),
        3
    );
    assert!(
        store.command("qroc-domain-advance", &a).is_err(),
        "stale expected domain must block"
    );
    let selected = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    assert_eq!(
        selected["domains"][1]["status"], "predicted",
        "selection alone generates nothing"
    );
    store.command("qroc-document-save", &json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"MARKET_ANALYSIS","source_revision":0,"content":"Durable test draft, revisable."}))?;
    let produced = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    assert_eq!(produced["domains"][1]["status"], "produced");
    assert!(
        !produced["domains"][1]["predicted_items"]
            .as_array()
            .expect("remaining catalog")
            .is_empty()
    );
    assert!(
        !produced["domains"][1]["predicted_items"]
            .as_array()
            .unwrap()
            .contains(&json!("MARKET_ANALYSIS"))
    );
    let corrected = store.command("qroc-document-save", &json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"MARKET_ANALYSIS","source_revision":1,"content":"Corrected durable fixture, revision two."}))?;
    assert_eq!(corrected["revision"], 2);
    drop(store);
    let mut store = QrocStore::open(&root)?;
    let after = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    assert_eq!(ids(&after), ORDER);
    assert_eq!(after["domains"][0]["status"], "produced");
    assert_eq!(after["domains"][1]["status"], "produced");
    for d in after["domains"]
        .as_array()
        .expect("overview")
        .iter()
        .skip(2)
    {
        assert_eq!(d["status"], "predicted");
    }
    a["expected_domain"] = json!("DISTRIBUTION");
    a["target_domain"] = json!("CAPITAL");
    assert!(
        store.command("qroc-domain-advance", &a).is_err(),
        "cannot skip predicted domains"
    );
    let docs = store.command(
        "qroc-document-list",
        &json!({"project_id":p["id"],"branch_id":b["id"]}),
    )?;
    assert_eq!(docs["domains"], after["domains"]);
    println!("ADVANCED_DOMAINS={}", after["domains"]);
    Ok(())
}

#[test]
fn legacy_fixture_opens_without_changing_document_records() -> Result<()> {
    let root = root("legacy-owned")?;
    let mut seed = QrocStore::open(&root)?;
    let p = seed.command(
        "qroc-project-create",
        &json!({"title":"Owned legacy fixture","brain_dump":"Fixture"}),
    )?;
    hardening::pass_hardening(&root);
    let b = seed.command(
        "qroc-branch-create",
        &json!({"project_id":p["id"],"name":"Fixture"}),
    )?;
    seed.command("qroc-document-save", &json!({"project_id":p["id"],"branch_id":b["id"],"document_type":"PRD","source_revision":0,"content":"Retained old draft"}))?;
    drop(seed);
    let mut store = QrocStore::open(&root)?;
    let projects = store.command("qroc-project-list", &json!({}))?;
    let p = &projects["projects"][0];
    let detail = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    let mut before = Vec::new();
    for b in detail["branches"].as_array().expect("fixture branches") {
        before.push(
            store.command(
                "qroc-document-list",
                &json!({"project_id":p["id"],"branch_id":b["id"]}),
            )?["documents"]
                .clone(),
        );
    }
    drop(store);
    let mut store = QrocStore::open(&root)?;
    let mut after = Vec::new();
    for b in detail["branches"].as_array().expect("fixture branches") {
        after.push(
            store.command(
                "qroc-document-list",
                &json!({"project_id":p["id"],"branch_id":b["id"]}),
            )?["documents"]
                .clone(),
        );
    }
    assert_eq!(before, after);
    let count: usize = before
        .iter()
        .map(|d| d.as_array().expect("documents").len())
        .sum();
    assert!(count > 0);
    println!("LEGACY_DOCUMENT_COUNT_BEFORE={count} AFTER={count}");
    Ok(())
}

#[test]
fn legacy_aliases_and_documents_map_without_rewriting_project_files() -> Result<()> {
    for (legacy, canonical) in [
        ("BUSINESS", "DISTRIBUTION"),
        ("PRESENCE", "BRAND"),
        ("LEGAL", "ECOSYSTEM"),
    ] {
        let root = root(legacy)?;
        let mut store = QrocStore::open(&root)?;
        let mut p = store.command(
            "qroc-project-create",
            &json!({"title":"Legacy alias fixture", "brain_dump":"Test fixture"}),
        )?;
        drop(store);
        // A fixture representing an old persisted project, never a real review.
        p["current_domain"] = json!(legacy);
        let path = root
            .join("projects")
            .join(format!("{}.json", p["id"].as_str().expect("project id")));
        let bytes = serde_json::to_vec_pretty(&p)?;
        fs::write(&path, &bytes)?;
        let mut store = QrocStore::open(&root)?;
        let detail = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
        assert_eq!(ids(&detail), ORDER);
        assert_eq!(detail["project"]["current_domain"], legacy);
        assert_eq!(
            detail["domains"]
                .as_array()
                .expect("domains")
                .iter()
                .find(|d| d["id"] == canonical)
                .expect("mapped domain")["status"],
            "predicted"
        );
        assert_eq!(fs::read(path)?, bytes);
    }
    for (kind, domain) in [
        ("PRD", "PRODUCT"),
        ("GO_TO_MARKET", "DISTRIBUTION"),
        ("LICENSING", "ECOSYSTEM"),
        ("BRAND_GUIDE", "BRAND"),
        ("COMPLIANCE_PRIVACY", "DATA"),
        ("PR_POLICY", "COMMUNITY"),
        ("FINANCIAL_OVERVIEW", "CAPITAL"),
    ] {
        assert_eq!(qroc_standalone::registry::entry(kind)?["domain"], domain);
    }
    qroc_standalone::registry::semantic_profile()?;
    Ok(())
}

#[test]
fn every_domain_traces_to_its_own_folder_with_its_documents() -> Result<()> {
    let root = root("folders")?;
    let mut store = QrocStore::open(&root)?;
    let p = store.command(
        "qroc-project-create",
        &json!({"title":"Seven folders", "brain_dump":"Test fixture only"}),
    )?;
    hardening::pass_hardening(&root);
    let b = store.command(
        "qroc-branch-create",
        &json!({"project_id":p["id"],"name":"Fixture"}),
    )?;
    // One document of each domain, by the domain the registry gives it.
    let one_each = [
        ("PRODUCT", "PRD", "PRD"),
        ("DISTRIBUTION", "BUSINESS_PLAN", "Business Plan"),
        ("ECOSYSTEM", "TERMS", "Terms"),
        ("BRAND", "BRAND_GUIDE", "Brand Guide"),
        ("DATA", "COMPLIANCE_PRIVACY", "Compliance Privacy"),
        ("COMMUNITY", "PR_POLICY", "PR Policy"),
        ("CAPITAL", "FINANCIAL_OVERVIEW", "Financial Overview"),
    ];
    for (domain, kind, _) in one_each {
        assert_eq!(
            qroc_standalone::registry::entry(kind)?["domain"],
            domain,
            "{kind}"
        );
        store.command("qroc-document-save", &json!({"project_id":p["id"],"branch_id":b["id"],"document_type":kind,"source_revision":0,"content":format!("Fixture {kind}")}))?;
    }
    let detail = store.command("qroc-project-detail", &json!({"id":p["id"]}))?;
    let domains = detail["domains"].as_array().expect("overview");
    let folders = domains
        .iter()
        .map(|d| d["folder"].as_str().expect("each domain names its folder"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(folders.len(), 7, "seven distinct folders");
    let project_folder = root.join("project-folders").join("Seven folders");
    for (domain, kind, file) in one_each {
        let d = domains.iter().find(|d| d["id"] == domain).expect("domain");
        let folder = project_folder.join(d["folder"].as_str().unwrap());
        assert!(folder.is_dir(), "{domain} folder");
        assert_eq!(
            fs::read_to_string(folder.join(format!("{file}.md")))?,
            format!("Fixture {kind}"),
            "{domain} holds its own document"
        );
    }
    for (legacy, folder) in [
        ("PRODUCT", "product"),
        ("DISTRIBUTION", "business"),
        ("BRAND", "presence"),
        ("ECOSYSTEM", "legal"),
    ] {
        let d = domains.iter().find(|d| d["id"] == legacy).unwrap();
        assert_eq!(d["folder"], folder, "earlier folder names stay");
    }
    let map = fs::read_to_string(project_folder.join("Domains.md"))?;
    for d in domains {
        assert!(map.contains(&format!("`{}/`", d["folder"].as_str().unwrap())));
    }
    println!("DOMAIN_FOLDERS={folders:?}");
    Ok(())
}

#[test]
fn creative_project_links_to_an_earlier_product_or_starts_alone_across_reopen() -> Result<()> {
    let root = root("creative-link")?;
    let mut store = QrocStore::open(&root)?;
    let product = store.command(
        "qroc-project-create",
        &json!({"title":"Earlier product", "brain_dump":"Test fixture only"}),
    )?;
    let linked = store.command(
        "qroc-project-create",
        &json!({"title":"Launch film","brain_dump":"A film for the product","kind":"media","linked_project_id":product["id"]}),
    )?;
    let alone = store.command(
        "qroc-project-create",
        &json!({"title":"Ad hoc track","brain_dump":"A track of its own","kind":"media"}),
    )?;
    assert_eq!(linked["linked_project_id"], product["id"]);
    assert!(alone.get("linked_project_id").is_none());
    for (args, refusal) in [
        (
            json!({"title":"Bad","brain_dump":"x","kind":"media","linked_project_id":alone["id"]}),
            "not another creative one",
        ),
        (
            json!({"title":"Bad","brain_dump":"x","kind":"media","linked_project_id":"missing"}),
            "was not found",
        ),
        (
            json!({"title":"Bad","brain_dump":"x","linked_project_id":product["id"]}),
            "Only a creative project",
        ),
    ] {
        let error = store
            .command("qroc-project-create", &args)
            .expect_err("refused");
        assert!(error.to_string().contains(refusal), "{error}");
    }
    let alone_file = root
        .join("projects")
        .join(format!("{}.json", alone["id"].as_str().unwrap()));
    let alone_bytes = fs::read(&alone_file)?;
    drop(store);
    let mut store = QrocStore::open(&root)?;
    let film = store.command("qroc-project-detail", &json!({"id":linked["id"]}))?;
    assert_eq!(film["project"]["linked_project_id"], product["id"]);
    assert_eq!(film["linked_project"]["title"], "Earlier product");
    assert_eq!(film["linked_project"]["found"], true);
    let own = store.command("qroc-project-detail", &json!({"id":alone["id"]}))?;
    assert_eq!(own["linked_project"], Value::Null);
    assert_eq!(
        fs::read(&alone_file)?,
        alone_bytes,
        "reopen rewrites nothing"
    );
    let earlier = store.command("qroc-project-detail", &json!({"id":product["id"]}))?;
    let followers = earlier["linked_creative_projects"].as_array().unwrap();
    assert_eq!(followers.len(), 1);
    assert_eq!(followers[0]["title"], "Launch film");
    // The link can be set later or cleared, and the change persists.
    store.command(
        "qroc-project-link",
        &json!({"project_id":alone["id"],"linked_project_id":product["id"]}),
    )?;
    store.command(
        "qroc-project-link",
        &json!({"project_id":linked["id"],"linked_project_id":null}),
    )?;
    assert!(
        store
            .command(
                "qroc-project-link",
                &json!({"project_id":product["id"],"linked_project_id":product["id"]}),
            )
            .is_err()
    );
    drop(store);
    let mut store = QrocStore::open(&root)?;
    let earlier = store.command("qroc-project-detail", &json!({"id":product["id"]}))?;
    let followers = earlier["linked_creative_projects"].as_array().unwrap();
    assert_eq!(followers.len(), 1);
    assert_eq!(followers[0]["title"], "Ad hoc track");
    let film = store.command("qroc-project-detail", &json!({"id":linked["id"]}))?;
    assert_eq!(film["linked_project"], Value::Null);
    let listed = store.command("qroc-project-list", &json!({}))?;
    let track = listed["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == alone["id"])
        .unwrap();
    assert_eq!(track["linked_project_id"], product["id"]);
    println!("CREATIVE_LINK={}", film["project"]);
    Ok(())
}
