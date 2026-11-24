//! Resumable whole-project orchestration over the existing hardening, graph and tail engines.
//! The journal is a project document; it never supplies scores or gate approvals.
use crate::{
    Error, Result, contracts,
    harden::{Hardening, ask_questions},
    qroc::QrocStore,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const USAGE: &str = "qroc-run --root ABSOLUTE_ROOT --project ID [--from hardening|gate|contracts|phod|product] [--serial] (answers: one JSON array on stdin when requested)";
/// Canonical engine inputs stay engine-owned. These describe the five orchestration boundaries.
pub const STEPS: [(&str, &str, &str); 5] = [
    (
        "hardening",
        "project inbox; person answers; selected route",
        "hardening rounds, questions, judged answers",
    ),
    (
        "gate",
        "passed hardening; canonical Product planning graph",
        "planning documents; Gate 1–3 verdicts and bounded revise loops",
    ),
    (
        "contracts",
        "hardened idea; current architecture and slice index; passed Gate 3",
        "Component contracts and bounded tail specs",
    ),
    (
        "phod",
        "each supported contract's bounded tail spec",
        "PHOD normalized specification and invocation receipt",
    ),
    (
        "product",
        "passed Gate 3; completed tail",
        "independent remaining Product documents; later domains predicted",
    ),
];
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn io_at(path: &Path, error: std::io::Error) -> Error {
    invalid(format!("{}: {error}", path.display()))
}
fn read(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).map_err(|e| io_at(path, e))?).map_err(Error::from)
}
fn save(path: &Path, value: &Value) -> Result<()> {
    crate::scope::validate_root(path)?;
    let temporary = path.with_extension("pipeline-tmp");
    crate::scope::validate_root(&temporary)?;
    let mut file = fs::File::create(&temporary).map_err(|e| io_at(&temporary, e))?;
    file.write_all(&serde_json::to_vec_pretty(value)?)
        .map_err(|e| io_at(&temporary, e))?;
    file.sync_all().map_err(|e| io_at(&temporary, e))?;
    fs::rename(&temporary, path).map_err(|e| io_at(path, e))?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| io_at(parent, e))?;
    }
    Ok(())
}
fn now() -> Result<u64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("System clock precedes epoch"))?
        .as_millis();
    u64::try_from(ms).map_err(|_| invalid("Timestamp exceeds supported range"))
}
fn identity<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 200
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        })
        .ok_or_else(|| invalid(format!("Invalid {key}")))
}
fn latest(store: &mut QrocStore, owner: &Value, kind: &str) -> Result<Option<Value>> {
    let list = store.command("qroc-document-list", owner)?;
    Ok(list["documents"]
        .as_array()
        .ok_or_else(|| invalid("Malformed document list"))?
        .iter()
        .filter(|d| d["document_type"] == kind)
        .max_by_key(|d| d["revision"].as_u64())
        .cloned())
}
fn jobs_idle(store: &mut QrocStore, owner: &Value) -> Result<()> {
    // Each generation has a bounded request deadline. A whole revise round may span
    // many such documents, so observe progress rather than imposing one 720s cap.
    loop {
        let jobs = store.command("qroc-job-list", owner)?;
        let active = jobs["jobs"]
            .as_array()
            .ok_or_else(|| invalid("Malformed job list"))?
            .iter()
            .filter(|j| j["branch_id"] == owner["branch_id"])
            .filter(|j| {
                matches!(
                    j["state"].as_str(),
                    Some("running" | "queued" | "cancelling")
                )
            })
            .collect::<Vec<_>>();
        if active.is_empty() {
            return Ok(());
        }
        let timestamp = now()? / 1000;
        for job in active.iter().filter(|j| j["state"] != "queued") {
            let started = job["started_at"]
                .as_u64()
                .ok_or_else(|| invalid("Job timestamp missing"))?;
            let budget = job["timeout_seconds"].as_u64().unwrap_or(120);
            if timestamp.saturating_sub(started) > budget + 30 {
                return Err(invalid(format!(
                    "Generation {} exceeded its bounded wait; checkpoint retained",
                    job["id"]
                )));
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn batch(store: &mut QrocStore, owner: &Value, kinds: &[&str], serial: bool) -> Result<Value> {
    jobs_idle(store, owner)?;
    let mut missing = Vec::new();
    for kind in kinds {
        match latest(store, owner, kind)? {
            Some(doc)
                if doc["generation_finish_reason"]
                    .as_str()
                    .is_none_or(|r| r == "stop") => {}
            Some(_) => {
                return Err(invalid(format!(
                    "{kind} is partial; inspect the retained draft"
                )));
            }
            None => missing.push(*kind),
        }
    }
    if missing.is_empty() {
        return Ok(json!({"resumed":true,"documents":kinds}));
    }
    let mut args = owner.clone();
    args["document_types"] = json!(missing);
    args["mode"] = json!(if serial { "chain" } else { "graph" });
    args["width"] = json!(3);
    let started = store.command("qroc-batch-start", &args)?;
    jobs_idle(store, owner)?;
    for job in started["jobs"]
        .as_array()
        .ok_or_else(|| invalid("Malformed batch jobs"))?
    {
        let done = store.command("qroc-job-status", &json!({"id":job["id"]}))?;
        if done["state"] != "succeeded" {
            return Err(invalid(format!("Product batch did not complete: {done}")));
        }
    }
    Ok(started)
}
fn gates(store: &mut QrocStore, owner: &Value, serial: bool) -> Result<Value> {
    let stages: [(&str, &[&str]); 3] = [
        (
            "GATE_1",
            &[
                "STRATEGY_BRIEF",
                "PRE_MORTEM",
                "PRD",
                "VALIDATION_PLAN",
                "TECHNICAL_ARCHITECTURE",
                "GATE_1",
            ],
        ),
        (
            "GATE_2",
            &[
                "DESIGN_ALTERNATIVES",
                "UI_UX_SPEC",
                "ROADMAP",
                "IMPLEMENTATION_PLAN",
                "GATE_2",
            ],
        ),
        ("GATE_3", &["EPICS", "SLICE_INDEX", "GATE_3"]),
    ];
    let mut records = Vec::new();
    for (gate, kinds) in stages {
        batch(store, owner, kinds, serial)?;
        let mut args = owner.clone();
        args["document_type"] = json!(gate);
        args["gate"] = json!(gate);
        args["serial"] = json!(serial);
        let document =
            latest(store, owner, gate)?.ok_or_else(|| invalid("Gate document disappeared"))?;
        args["revision"] = document["revision"].clone();
        let before = store.command("qroc-gate-evaluate", &args)?;
        let mut revise = store.command("qroc-gate-revise-get", &args)?;
        if before["status"] != "passed" {
            // A press is bounded by the existing engine. Never silently start another exhausted press.
            // The run he started is the press of its first round; every later round is
            // his own press in the window (design record entry 42).
            if revise["latest"]["awaiting_press"] == true {
                return Err(waiting_for_press(gate, &revise));
            }
            if revise["latest"].is_null()
                || matches!(
                    revise["latest"]["state"].as_str(),
                    Some("running" | "blocked")
                )
            {
                store.command("qroc-gate-revise-start", &args)?;
            } else if revise["latest"]["state"] == "failed" {
                return Err(invalid(format!(
                    "{gate} exhausted its bounded revise loop: {}",
                    revise["latest"]
                )));
            }
            jobs_idle(store, owner)?;
            revise = store.command("qroc-gate-revise-get", &args)?;
            if revise["latest"]["awaiting_press"] == true {
                return Err(waiting_for_press(gate, &revise));
            }
        }
        let document =
            latest(store, owner, gate)?.ok_or_else(|| invalid("Revised gate disappeared"))?;
        args["revision"] = document["revision"].clone();
        let verdict = store.command("qroc-gate-evaluate", &args)?;
        if verdict["status"] != "passed" {
            return Err(invalid(format!("{gate} has not passed: {verdict}")));
        }
        records.push(json!({"gate":gate,"document_id":document["id"],"revision":document["revision"],"before":before,"verdict":verdict,"revise":revise}));
    }
    Ok(json!({"gates":records,"graph":store.graph_export(owner)?}))
}
fn waiting_for_press(gate: &str, revise: &Value) -> crate::Error {
    invalid(format!(
        "{gate} failed round {} of {}; the next round waits for the person's press: {}",
        revise["latest"]["rounds"].as_array().map_or(0, Vec::len),
        revise["max_rounds"],
        revise["latest"]
    ))
}
fn harden(store: &mut QrocStore, project: &str, args: &Value) -> Result<Value> {
    let root = store.root().to_path_buf();
    let hardening = Hardening::open(&root)?;
    let mut record = hardening.load(project)?;
    if record.rounds.is_empty() {
        let inbox = root.join("inbox").join(project);
        let mut sources = fs::read_dir(&inbox)
            .map_err(|e| io_at(&inbox, e))?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|e| io_at(&inbox, e))?;
        sources.sort();
        let first = sources
            .iter()
            .find(|p| p.extension().is_some_and(|e| e == "json"))
            .ok_or_else(|| {
                invalid("Project inbox has no source; capture material before running")
            })?;
        let source = read(first)?;
        let original = source["source"]
            .as_str()
            .ok_or_else(|| invalid("Inbox source has no source identity"))?;
        let kind = if source["kind"] == "note" {
            if Path::new(original).is_absolute() {
                "file"
            } else {
                "text"
            }
        } else {
            source["kind"]
                .as_str()
                .ok_or_else(|| invalid("Inbox source has no kind"))?
        };
        crate::intake::command(
            store,
            &json!({"project_id":project,"source":original,"kind":kind,"start_hardening":true}),
        )?;
        record = hardening.load(project)?;
    }
    if !hardening.passed(project)?
        && let Some(answers) = args.get("answers")
    {
        let answers = answers
            .as_array()
            .filter(|a| {
                !a.is_empty()
                    && a.iter()
                        .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
            })
            .ok_or_else(|| invalid("Answers must be a JSON array of nonempty strings"))?;
        record = hardening.answer(project, &serde_json::to_string(answers)?, &|i, s| {
            ask_questions(&root, i, s)
        })?;
    }
    Ok(serde_json::to_value(record)?)
}
fn tail(store: &QrocStore, project: &str, branch: &str) -> Result<Value> {
    let input = store.tail_input(project, branch)?;
    let base = contracts::tail_base(store.root(), project, branch)?;
    if let Some(dir) = contracts::latest_revision(&base)?
        && read(&dir.join("state.json"))?["nodes"]["CONSISTENCY_JOIN"] == "produced"
    {
        // Recover the durable engine outputs if the process ended before journal publication.
        let mut contracts = Vec::new();
        let mut specs = Vec::new();
        let mut unsupported = Vec::new();
        for i in 0..64 {
            let path = dir.join(format!("contracts/component-{i}.json"));
            if !path.exists() {
                break;
            }
            let c = read(&path)?["contract"].clone();
            let path = dir.join(format!("specs/component-{i}.json"));
            if path.exists() {
                specs.push(json!({"index":i,"name":c["name"],"spec":read(&path)?}));
            } else {
                unsupported.push(
                    json!({"index":i,"name":c["name"],"status":"unsupported; inspect contract"}),
                );
            }
            contracts.push(c);
        }
        return Ok(
            json!({"revision":read(&dir.join("state.json"))?["revision"],"contracts":contracts,"specs":specs,"unsupported":unsupported,"join":read(&dir.join("join.json"))?}),
        );
    }
    contracts::run_tail(store.root(), &input)
}
fn phod(root: &Path, project: &str, branch: &str, tail: &Value, directory: &Path) -> Result<Value> {
    let checker = std::env::var_os("QROC_PHOD_ADMITTED_BIN")
        .or_else(|| std::env::var_os("PHOD_SEMANTIC_BIN"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("QROC_RESOURCE_ROOT")
                .map(|p| PathBuf::from(p).join("runtime/phod/bin/phod-admitted"))
        })
        .ok_or_else(|| {
            invalid(
                "PHOD checker unavailable; configure QROC_PHOD_ADMITTED_BIN or QROC_RESOURCE_ROOT",
            )
        })?;
    if !checker.is_absolute() || !checker.is_file() {
        return Err(invalid(
            "PHOD checker must name an absolute existing binary",
        ));
    }
    let bytes = fs::read(&checker).map_err(|e| io_at(&checker, e))?;
    let tool_hash = contracts::hash(&bytes);
    if let Ok(expected) = std::env::var("QROC_PHOD_ADMITTED_SHA256")
        && expected != tool_hash
    {
        return Err(invalid(
            "PHOD checker identity differs from the selected digest",
        ));
    }
    let base = contracts::tail_base(root, project, branch)?;
    let revision = tail["revision"]
        .as_u64()
        .ok_or_else(|| invalid("Tail revision missing"))?;
    let mut specs = Vec::new();
    for entry in tail["specs"]
        .as_array()
        .ok_or_else(|| invalid("Tail spec list missing"))?
    {
        let index = entry["index"]
            .as_u64()
            .ok_or_else(|| invalid("Spec index missing"))?;
        let source = base.join(format!("r{revision}/specs/component-{index}.json"));
        let destination = directory.join(format!("spec-{index}.json"));
        let receipt = if destination.exists() {
            let receipt = read(&destination)?;
            if receipt["invocation"]["checker_sha256"] != tool_hash
                || receipt["invocation"]["source_sha256"]
                    != contracts::hash(&fs::read(&source).map_err(|e| io_at(&source, e))?)
            {
                return Err(invalid(
                    "Persisted PHOD invocation no longer matches its inputs",
                ));
            }
            receipt
        } else {
            let started = now()?;
            let output = Command::new("/usr/bin/timeout")
                .arg("30")
                .arg(&checker)
                .args(["normalize", "record"])
                .arg(&source)
                .current_dir(directory)
                .stdin(Stdio::null())
                .output()
                .map_err(|e| io_at(&checker, e))?;
            let ended = now()?;
            if !output.status.success() {
                return Err(invalid(format!(
                    "PHOD normalization failed ({}): {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            if output.stdout.len() > 4 * 1024 * 1024 {
                return Err(invalid("PHOD output exceeds 4 MiB"));
            }
            let normalized: Value = serde_json::from_slice(&output.stdout)?;
            let receipt = json!({"id":format!("{project}-{branch}-phod-{index}"),"name":entry["name"],"contract_index":index,"spec":normalized,"standing":"specification only; no fulfilment or execution approval","invocation":{"argv":[checker.to_string_lossy(),"normalize","record",source.to_string_lossy()],"checker_sha256":tool_hash,"source_sha256":contracts::hash(&fs::read(&source).map_err(|e|io_at(&source,e))?),"started":started,"ended":ended,"exit_code":output.status.code(),"stdout_sha256":contracts::hash(&output.stdout)}});
            save(&destination, &receipt)?;
            receipt
        };
        specs.push(receipt);
    }
    Ok(json!({"specs":specs,"unsupported":tail["unsupported"]}))
}
fn domains(store: &mut QrocStore, owner: &Value, record: &Value) -> Result<Value> {
    let documents = store.command("qroc-document-list", owner)?;
    let docs = documents["documents"]
        .as_array()
        .ok_or_else(|| invalid("Malformed document list"))?;
    let mut gates_passed = record["status"] == "completed";
    if gates_passed {
        for gate in ["GATE_1", "GATE_2", "GATE_3"] {
            let Some(document) = latest(store, owner, gate)? else {
                gates_passed = false;
                break;
            };
            let mut args = owner.clone();
            args["document_type"] = json!(gate);
            args["revision"] = document["revision"].clone();
            gates_passed &= store.command("qroc-gate-evaluate", &args)?["status"] == "passed";
        }
    }
    let mut result = json!({});
    for domain in ["PRODUCT", "BUSINESS", "PRESENCE", "LEGAL"] {
        let kinds = docs
            .iter()
            .filter(|d| {
                d["document_type"]
                    .as_str()
                    .is_some_and(|k| crate::qroc::domain_for(k) == domain)
            })
            .filter_map(|d| d["document_type"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let complete = !kinds.is_empty() && (domain != "PRODUCT" || gates_passed);
        result[domain] = json!({"state":if complete{"produced"}else{"predicted"},"produced":if complete{kinds.len()}else{0},"documents":kinds});
    }
    Ok(result)
}
fn reply(store: &mut QrocStore, record: &Value, path: &Path, resumed: bool) -> Result<Value> {
    let owner = json!({"project_id":record["project_id"],"branch_id":record["branch_id"]});
    let project = identity(record, "project_id")?;
    let folder = store.open_project_folder(project)?;
    let document = folder.join("Pipeline run.json");
    save(&document, record)?;
    let spec_document = folder.join("PHOD specifications.json");
    if record["steps"][3]["status"] == "completed" {
        save(&spec_document, &record["steps"][3]["output"])?;
    }
    let mut result = json!({"spec_document_path":spec_document,"record":record,"record_path":path,"document_path":document,"domains":domains(store,&owner,record)?,"resumed":resumed});
    if record["status"] == "waiting_answers" {
        result["questions"] = record["steps"][0]["output"]["rounds"]
            .as_array()
            .and_then(|r| r.last())
            .map(|r| r["questions"]["items"].clone())
            .unwrap_or(json!([]));
    }
    Ok(result)
}
/// The bridge arm and CLI call this same store-owned orchestration boundary.
pub fn command(store: &mut QrocStore, args: &Value) -> Result<Value> {
    let project = identity(args, "project_id")?.to_owned();
    crate::scope::validate_root(store.root())?;
    store.project(&project)?;
    if store.project(&project)?.kind != "product" {
        return Err(invalid("Whole pipeline requires a product project"));
    }
    if crate::routes::selected_at(store.root())?
        .is_some_and(|r| r.kind == crate::routes::Kind::Openrouter)
    {
        return Err(invalid(
            "Automatic pipeline requires a selected local or relay route",
        ));
    }
    let route = crate::registry::provider_config_at(store.root())?;
    crate::routes::guard_endpoint(&route.0)?;
    store.validate_pipeline_budget()?;
    let route_identity = crate::routes::logical_identity(store.root())?;
    let directory = store.root().join("pipeline").join(&project);
    crate::scope::validate_root(&directory)?;
    fs::create_dir_all(&directory).map_err(|e| io_at(&directory, e))?;
    let lock_path = directory.join(".lock");
    crate::scope::validate_root(&lock_path)?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| io_at(&lock_path, e))?;
    lock.try_lock()
        .map_err(|e| invalid(format!("Pipeline busy for {project}: {e}")))?;
    let path = directory.join("Pipeline run.json");
    let resumed = path.exists();
    let from = args
        .get("from")
        .map(|v| {
            v.as_str()
                .ok_or_else(|| invalid("from must be a step name"))
        })
        .transpose()?;
    let from_index = from
        .map(|name| {
            STEPS
                .iter()
                .position(|s| s.0 == name)
                .ok_or_else(|| invalid(USAGE))
        })
        .transpose()?;
    if !resumed && from_index.is_some_and(|i| i > 0) {
        return Err(invalid(
            "Cannot skip an incomplete predecessor; resume from the last completed step",
        ));
    }
    let serial = args
        .get("serial")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| invalid("serial must be a boolean"))
        })
        .transpose()?
        .unwrap_or(false);
    let mut record = if resumed {
        read(&path)?
    } else {
        let branch=store.command("qroc-branch-create",&json!({"project_id":project,"name":"Whole pipeline","provider":crate::registry::provider_name_at(store.root())?,"model":route.1}))?;
        json!({"schema":"qroc-pipeline/1","project_id":project,"branch_id":branch["id"],"route":route_identity,"serial":serial,"status":"running","steps":STEPS.iter().map(|(step,inputs,outputs)|json!({"step":step,"inputs":inputs,"outputs":outputs,"status":"waiting","started":null,"ended":null,"route":route_identity,"result_id":null,"output":null})).collect::<Vec<_>>()})
    };
    // Older journals recorded a transient endpoint. Migrate only a matching named
    // relay branch; local endpoints remain part of identity.
    if resumed && record["route"].get("name").is_none() {
        let branch = store.command("qroc-branch-list", &json!({"project_id":project}))?;
        let original = branch["branches"]
            .as_array()
            .and_then(|items| items.iter().find(|b| b["id"] == record["branch_id"]));
        if original.is_some_and(|b| {
            b["provider"] == route_identity["name"] && b["model"] == route_identity["model"]
        }) && record["route"]["model"] == route_identity["model"]
            && (route_identity["kind"] == "relay"
                || record["route"]["endpoint"] == route_identity["endpoint"])
        {
            record["route"] = route_identity.clone();
        }
    }
    if record["schema"] != "qroc-pipeline/1"
        || record["project_id"] != project
        || record["route"] != route_identity
    {
        return Err(invalid(
            "Pipeline identity or route changed; continue on its original route",
        ));
    }
    if args.get("serial").is_some() && record["serial"] != serial {
        return Err(invalid(
            "Pipeline execution mode changed; use a separate project copy to measure",
        ));
    }
    let serial = record["serial"]
        .as_bool()
        .ok_or_else(|| invalid("Pipeline mode missing"))?;
    if record["steps"].as_array().is_none_or(|steps| {
        steps.len() != STEPS.len()
            || steps.iter().zip(STEPS).any(|(entry, step)| {
                entry["step"] != step.0 || !entry.is_object() || !entry["status"].is_string()
            })
    }) {
        return Err(invalid("Malformed pipeline step journal"));
    }
    if let Some(index) = from_index
        && (0..index).any(|i| record["steps"][i]["status"] != "completed")
    {
        return Err(invalid(
            "Cannot skip an incomplete predecessor; resume from the last completed step",
        ));
    }
    if record["status"] == "completed" {
        if !Hardening::open(store.root())?.passed(&project)? {
            return Err(invalid(
                "Project idea changed since this pipeline completed",
            ));
        }
        return reply(store, &record, &path, true);
    }
    let branch = identity(&record, "branch_id")?.to_owned();
    let owner = json!({"project_id":project,"branch_id":branch});
    for (index, (step, _, _)) in STEPS.iter().enumerate() {
        if record["steps"][index]["status"] == "completed" {
            continue;
        }
        if record["steps"][index]["started"].is_null() {
            record["steps"][index]["started"] = json!(now()?);
        }
        record["status"] = json!("running");
        record["steps"][index]["status"] = json!("running");
        save(&path, &record)?;
        let outcome = (|| -> Result<Value> {
            match *step {
                "hardening" => harden(store, &project, args),
                "gate" => gates(store, &owner, serial),
                "contracts" => tail(store, &project, &branch),
                "phod" => phod(
                    store.root(),
                    &project,
                    &branch,
                    &record["steps"][2]["output"],
                    &directory,
                ),
                "product" => {
                    let batch = batch(store, &owner, &["AGENT_DIRECTIVES", "TASKS"], serial)?;
                    Ok(json!({"batch":batch,"graph":store.graph_export(&owner)?}))
                }
                _ => Err(invalid("Unknown pipeline step")),
            }
        })();
        let output = match outcome {
            Ok(v) => v,
            Err(error) => {
                record["status"] = json!("failed");
                record["steps"][index]["status"] = json!("failed");
                record["steps"][index]["error"] = json!(error.to_string());
                record["domains"] = domains(store, &owner, &record)?;
                save(&path, &record)?;
                return Err(error);
            }
        };
        record["steps"][index]["output"] = output;
        if *step == "hardening" && !Hardening::open(store.root())?.passed(&project)? {
            record["status"] = json!("waiting_answers");
            record["steps"][index]["status"] = json!("waiting_answers");
            save(&path, &record)?;
            return reply(store, &record, &path, resumed);
        }
        record["steps"][index]["status"] = json!("completed");
        record["steps"][index]["error"] = Value::Null;
        record["steps"][index]["ended"] = json!(now()?);
        record["steps"][index]["result_id"] = match *step {
            "hardening" => json!(
                store
                    .root()
                    .join("hardening")
                    .join(format!("{project}.json"))
            ),
            "gate" => record["steps"][index]["output"]["gates"][2]["document_id"].clone(),
            "contracts" => json!(
                contracts::tail_base(store.root(), &project, &branch)?
                    .join(format!("r{}", record["steps"][index]["output"]["revision"]))
            ),
            "phod" => json!(directory),
            "product" => record["steps"][index]["output"]["batch"]["batch_id"]
                .as_str()
                .map(|s| json!(s))
                .unwrap_or(json!(
                    store.root().join("documents").join(&project).join(&branch)
                )),
            _ => return Err(invalid("Unknown step result")),
        };
        save(&path, &record)?;
    }
    record["status"] = json!("completed");
    record["domains"] = domains(store, &owner, &record)?;
    save(&path, &record)?;
    reply(store, &record, &path, resumed)
}
/// CLI parsing and question interaction live in the library; the binary only prints the result.
pub fn cli(args: &[String]) -> Result<Value> {
    let mut root = None;
    let mut request = json!({});
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--serial" => {
                if request.get("serial").is_some() {
                    return Err(invalid(USAGE));
                }
                request["serial"] = json!(true);
                i += 1;
            }
            "--root" | "--project" | "--from" => {
                let value = args.get(i + 1).ok_or_else(|| invalid(USAGE))?;
                match args[i].as_str() {
                    "--root" => {
                        if root.is_some() {
                            return Err(invalid(USAGE));
                        }
                        root = Some(PathBuf::from(value));
                    }
                    flag => {
                        let key = if flag == "--project" {
                            "project_id"
                        } else {
                            "from"
                        };
                        if request.get(key).is_some() {
                            return Err(invalid(USAGE));
                        }
                        request[key] = json!(value);
                    }
                }
                i += 2;
            }
            _ => return Err(invalid(USAGE)),
        }
    }
    let root = root.ok_or_else(|| invalid(USAGE))?;
    crate::scope::validate_root(&root)?;
    // Use the bridge's ownership lock and identical container-root convention.
    let mut bridge = crate::bridge::Bridge::open(&root)?;
    let mut sequence = 0;
    loop {
        sequence += 1;
        let response=bridge.reply(json!({"version":1,"id":format!("pipeline-{sequence}"),"command":"qroc-run","args":request}));
        if response["ok"] != true {
            return Err(invalid(
                response["error"].as_str().unwrap_or("Pipeline failed"),
            ));
        }
        let result = response["result"].clone();
        if result["record"]["status"] != "waiting_answers" {
            return Ok(result);
        }
        eprintln!(
            "Hardening needs your answers (one JSON array, one answer per question): {}",
            result["questions"]
        );
        std::io::stderr().flush()?;
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(result);
        }
        request["answers"] = serde_json::from_str(&line)?;
    }
}
