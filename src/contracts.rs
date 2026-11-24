//! Revisioned Component contracts and bounded PHOD specs. No gate approval or execution.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, sync::atomic::AtomicBool};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordShape {
    pub text_field: String,
    pub integer_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub unknown_fields: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub name: String,
    pub purpose: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub behaviour: Vec<String>,
    pub errors: Vec<String>,
    pub effects: Vec<String>,
    pub profile: String,
    pub record: Option<RecordShape>,
    pub unsupported_reason: Option<String>,
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Exact supported profile only. Broader behaviour is never silently dropped.
pub fn spec(contract: &Contract) -> std::result::Result<Value, String> {
    if contract.profile != "qroc-record/1" {
        return Err(contract
            .unsupported_reason
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                "outside qroc-record/1: only pure bounded record inspection is supported".into()
            }));
    }
    if !contract.effects.is_empty() {
        return Err("qroc-record/1 cannot express effects".into());
    }
    let r = contract.record.as_ref().ok_or("record shape is missing")?;
    if contract.name.trim().is_empty()
        || r.text_field.trim().is_empty()
        || r.integer_field.trim().is_empty()
        || r.text_field == r.integer_field
        || r.minimum > r.maximum
        || !["reject", "preserve", "ignore"].contains(&r.unknown_fields.as_str())
    {
        return Err("invalid bounded record shape".into());
    }
    // The model must choose this profile only for this complete set of clauses.
    let expected = record_behaviour(r);
    if contract.behaviour != expected
        || contract.errors != ["reject invalid record"].map(str::to_owned)
    {
        return Err("additional or missing behaviour cannot be expressed by qroc-record/1".into());
    }
    Ok(
        json!({"profile":"qroc-record/1","name":contract.name,"text_field":r.text_field,"integer_field":r.integer_field,"minimum":r.minimum,"maximum":r.maximum,"unknown_fields":r.unknown_fields}),
    )
}
pub(crate) fn record_behaviour(r: &RecordShape) -> Vec<String> {
    vec![
        format!("nonempty text: {}", r.text_field),
        format!(
            "integer: {} in {}..{}",
            r.integer_field, r.minimum, r.maximum
        ),
        format!("unknown fields: {}", r.unknown_fields),
    ]
}
pub(crate) fn save(path: &Path, value: &Value) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    Ok(())
}
pub(crate) fn ask(
    root: &Path,
    endpoint: &str,
    model: &str,
    prompt: &str,
    evidence: &Path,
) -> Result<String> {
    crate::routes::guard_endpoint(endpoint)?;
    let request =
        json!({"model":model,"messages":[{"role":"user","content":prompt}],"stream":false});
    save(&evidence.with_extension("request.json"), &request)?;
    let (reply, _) =
        crate::routes::complete_at(root, endpoint, &request, 300, &AtomicBool::new(false))?;
    save(&evidence.with_extension("reply.json"), &reply)?;
    if reply["choices"][0]["finish_reason"] != "stop" {
        return Err(invalid("route response incomplete"));
    }
    reply["choices"][0]["message"]["content"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid("route returned empty content"))
}
fn parse_json(content: &str) -> Result<Value> {
    let s = content.trim();
    let s = if s.starts_with("```json\n") && s.ends_with("```") {
        &s[8..s.len() - 3]
    } else {
        s
    };
    Ok(serde_json::from_str(s)?)
}
/// The contract-design request of section 3.2: one contract per bounded part.
fn contracts_prompt(idea: &str, architecture: &str, slice_index: &str, source: &str) -> String {
    format!(
        r#"Design Component contracts, one per bounded part, from the hardened idea and Product architecture and slice index below. Return JSON only, an array of contracts. Every contract must have exactly: name, purpose, inputs (string array), outputs (string array), behaviour (string array), errors (string array), effects (string array), profile, record (object or null), unsupported_reason (string or null). Cover every bounded part in the slice index, including presentation generation. Do not pretend PHOD supports deck generation. The only supported profile qroc-record/1 is PURE inspection of an object with a nonempty string field and a bounded integer field, plus explicit unknown-field policy. Use it only if that is the ENTIRE component behaviour, with no effects. A genuine isolated scene metadata record validator may fit if architecture needs it; do not invent a component merely to get a supported spec. Its record object has exactly text_field, integer_field, minimum, maximum, unknown_fields (reject|preserve|ignore). For that profile behaviour must be exactly ["nonempty text: FIELD", "integer: FIELD in MIN..MAX", "unknown fields: POLICY"] and errors exactly ["reject invalid record"], with empty effects. Every other profile must be "unsupported", record null, and a plain specific unsupported_reason. No execution or approval claims. Input answers are {source}. Hardened idea: {idea}
Architecture: {architecture}
Slice index: {slice_index}"#
    )
}
fn parse_contracts(content: &str) -> Result<Vec<Contract>> {
    let contracts: Vec<Contract> = serde_json::from_value(parse_json(content)?)?;
    if contracts.is_empty() || contracts.len() > 64 {
        return Err(invalid("expected 1..64 component contracts"));
    }
    Ok(contracts)
}
/// A focused generation path: only the transitive inputs of architecture and slice index.
/// Gate decisions remain with the existing owner; these are drafts, never approvals.
pub fn run(input: &Path, root: &Path) -> Result<Value> {
    crate::scope::validate_root(root)?;
    let bytes = fs::read(input)?;
    let record: Value = serde_json::from_slice(&bytes)?;
    if record["schema"] != "qroc-hardening/1" {
        return Err(invalid("wrong hardening schema"));
    }
    let round = record["rounds"]
        .as_array()
        .and_then(|r| r.last())
        .ok_or_else(|| invalid("missing hardening round"))?;
    if round["verdict"] != "pass" || round["sent_back"] != false {
        return Err(invalid("idea has not passed hardening"));
    }
    let idea = round["idea_text"]
        .as_str()
        .ok_or_else(|| invalid("missing hardened idea"))?;
    if round["idea_sha256"] != hash(idea.as_bytes()) {
        return Err(invalid("hardened idea hash mismatch"));
    }
    let (endpoint, model) = crate::registry::provider_config_at(root)?;
    crate::routes::guard_endpoint(&endpoint)?;
    fs::create_dir_all(root.join("product/revisions"))?;
    let mut revision = 1;
    let dir = loop {
        let d = root.join(format!("product/revisions/r{revision}"));
        match fs::create_dir(&d) {
            Ok(()) => break d,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => revision += 1,
            Err(e) => return Err(e.into()),
        }
    };
    for folder in ["contracts", "specs", "evidence"] {
        fs::create_dir(dir.join(folder))?;
    }
    save(&dir.join("hardened-idea.json"), &record)?;
    let stages: [(&str, &[&str]); 9] = [
        ("Strategy brief", &[]),
        ("PRD", &["Strategy brief"]),
        ("Technical architecture", &["PRD"]),
        ("Design alternatives", &["PRD", "Technical architecture"]),
        ("Roadmap", &["PRD", "Technical architecture"]),
        ("UI and UX spec", &["PRD", "Design alternatives"]),
        (
            "Implementation plan",
            &["Technical architecture", "Roadmap"],
        ),
        ("Epics", &["PRD", "Implementation plan", "UI and UX spec"]),
        ("Slice index", &["Epics"]),
    ];
    let mut documents = BTreeMap::new();
    for (i, (name, dependencies)) in stages.iter().enumerate() {
        let context: BTreeMap<_, _> = dependencies.iter().map(|d| (*d, &documents[*d])).collect();
        let prompt = format!(
            "Write the complete Product document {name}. Draft only: do not claim gate approval, implementation or evidence. Preserve offline editable decks and motion shows, never flatten slides. The input is a T401 worker stand-in answer, not operator testimony. Read only the supplied dependencies; no forward references. Hardened idea (used directly only for Strategy brief): {}. Dependencies: {}",
            if dependencies.is_empty() {
                idea
            } else {
                "see dependencies"
            },
            serde_json::to_string(&context)?
        );
        let content = ask(
            root,
            &endpoint,
            &model,
            &prompt,
            &dir.join(format!("evidence/doc-{i}")),
        )?;
        save(
            &dir.join(format!("{name}.json")),
            &json!({"title":name,"revision":revision,"content":content,"sha256":hash(content.as_bytes()),"inputs":dependencies,"standing":"draft","gate_approval":false}),
        )?;
        println!("Document: {name} (revision {revision})");
        documents.insert(*name, content);
    }
    let content = ask(
        root,
        &endpoint,
        &model,
        &contracts_prompt(
            idea,
            &documents["Technical architecture"],
            &documents["Slice index"],
            "T401 stand-ins",
        ),
        &dir.join("evidence/contracts"),
    )?;
    let contracts = parse_contracts(&content)?;
    let mut names = std::collections::BTreeSet::new();
    let mut specs = Vec::new();
    let mut unsupported = Vec::new();
    for (i, c) in contracts.iter().enumerate() {
        if c.name.trim().is_empty()
            || !names.insert(&c.name)
            || c.purpose.trim().is_empty()
            || c.behaviour.is_empty()
        {
            return Err(invalid("invalid or duplicate component contract"));
        }
        save(
            &dir.join(format!("contracts/component-{i}.json")),
            &json!({"revision":revision,"contract":c,"inputs":{"idea_sha256":round["idea_sha256"],"architecture_sha256":hash(documents["Technical architecture"].as_bytes()),"slice_index_sha256":hash(documents["Slice index"].as_bytes())}}),
        )?;
        println!("Contract: {}", c.name);
        match spec(c) {
            Ok(s) => {
                let path = dir.join(format!("specs/component-{i}.json"));
                save(&path, &s)?;
                println!("Spec: {} -> {}", c.name, path.display());
                specs.push(json!({"name":c.name,"path":path}));
            }
            Err(reason) => {
                println!("{}: unsupported: {reason}", c.name);
                unsupported.push(json!({"name":c.name,"status":format!("unsupported: {reason}")}));
            }
        }
    }
    let prompt = format!(
        "Give PHOD readiness advice on these Component contracts and generated spec outcomes. Advisory only; never approve a gate, rewrite contracts, or claim validation has run. State unsupported limits plainly. Return concise plain text. {}",
        json!({"contracts":contracts,"specs":specs,"unsupported":unsupported})
    );
    let advice = ask(
        root,
        &endpoint,
        &model,
        &prompt,
        &dir.join("evidence/readiness"),
    )?;
    save(
        &dir.join("readiness.json"),
        &json!({"advisory":true,"endpoint":endpoint,"model":model,"content":advice}),
    )?;
    println!("Readiness advice: {advice}");
    let result = json!({"revision":revision,"standing":"draft; no gate approval","source_label":"T401 stand-in answers","source_sha256":hash(&bytes),"documents":documents.keys().collect::<Vec<_>>(),"contracts":contracts,"specs":specs,"unsupported":unsupported,"revision_dir":dir});
    save(&dir.join("summary.json"), &result)?;
    // Publish readable latest documents only after the complete revision exists.
    for (name, content) in &documents {
        let latest = root.join("product").join(format!("{name}.md"));
        let temporary = root
            .join("product")
            .join(format!(".{name}-r{revision}.tmp"));
        fs::write(&temporary, content)?;
        fs::rename(&temporary, &latest)?;
    }
    let latest = root.join("product/Component contracts.json");
    let temporary = root
        .join("product")
        .join(format!(".contracts-r{revision}.tmp"));
    fs::write(&temporary, serde_json::to_vec_pretty(&result)?)?;
    fs::rename(&temporary, &latest)?;
    println!(
        "Counts: contracts {} of {}; specs {} of {}; unsupported {} of {}",
        contracts.len(),
        contracts.len(),
        specs.len(),
        contracts.len(),
        unsupported.len(),
        contracts.len()
    );
    Ok(result)
}
/// The four tail nodes of the Product graph (section 3.2), in order.
pub const TAIL_NODES: [&str; 4] = [
    "COMPONENT_CONTRACTS",
    "PHOD_SPEC",
    "VERIFICATION",
    "CONSISTENCY_JOIN",
];
/// The step each tail node runs; the graph export names it.
pub fn tail_step(node: &str) -> Option<&'static str> {
    Some(match node {
        "COMPONENT_CONTRACTS" => "contracts: design one contract per bounded part (route)",
        "PHOD_SPEC" => "contracts: spec per contract under qroc-record/1, else unsupported",
        "VERIFICATION" => "contracts: verify each spec against its contract",
        "CONSISTENCY_JOIN" => {
            "contracts: readiness advice over every spec and verification (route)"
        }
        _ => return None,
    })
}

/// What the tail reads: the hardened idea and the branch's current architecture and slice index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailInput {
    pub project_id: String,
    pub branch_id: String,
    pub idea: String,
    pub idea_sha256: String,
    pub architecture: String,
    pub architecture_sha256: String,
    pub slice_index: String,
    pub slice_index_sha256: String,
}

pub(crate) fn tail_base(root: &Path, project: &str, branch: &str) -> Result<std::path::PathBuf> {
    for id in [project, branch] {
        if id.is_empty()
            || id.len() > 200
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(invalid("Invalid tail project/branch identity"));
        }
    }
    Ok(root.join("tail").join(project).join(branch))
}

/// Structural check inside Q-ROC: the spec is derived again from its contract and must be
/// identical. PHOD's own checker (`phod-semantic`) is not run by the engine.
pub fn verify(contract: &Contract, produced: &Value) -> Value {
    match spec(contract) {
        Ok(expected) if &expected == produced => {
            json!({"status":"verified","check":"spec re-derived from its contract under qroc-record/1 is identical","phod_checker_run":false})
        }
        Ok(_) => {
            json!({"status":"failed","reason":"spec differs from the one its contract gives","phod_checker_run":false})
        }
        Err(reason) => json!({"status":"failed","reason":reason,"phod_checker_run":false}),
    }
}

pub(crate) fn write_state(dir: &Path, state: &Value) -> Result<()> {
    let temporary = dir.join(".state.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    fs::rename(&temporary, dir.join("state.json"))?;
    Ok(())
}

/// Runs the tail for one branch: contracts from the route, then for each contract its spec (or
/// a plain unsupported reason) and that spec's verification, then the consistency join. Every
/// run is a new revision under `<root>/tail/<project>/<branch>/r<n>`; earlier ones are kept.
pub fn run_tail(root: &Path, input: &TailInput) -> Result<Value> {
    crate::scope::validate_root(root)?;
    let base = tail_base(root, &input.project_id, &input.branch_id)?;
    for (text, sha) in [
        (&input.idea, &input.idea_sha256),
        (&input.architecture, &input.architecture_sha256),
        (&input.slice_index, &input.slice_index_sha256),
    ] {
        if text.trim().is_empty() || &hash(text.as_bytes()) != sha {
            return Err(invalid("Tail input is empty or its digest does not match"));
        }
    }
    let (endpoint, model) = crate::registry::provider_config_at(root)?;
    fs::create_dir_all(&base)?;
    let mut revision = 1;
    let dir = loop {
        let d = base.join(format!("r{revision}"));
        match fs::create_dir(&d) {
            Ok(()) => break d,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => revision += 1,
            Err(e) => return Err(e.into()),
        }
    };
    for folder in ["contracts", "specs", "verifications", "evidence"] {
        fs::create_dir(dir.join(folder))?;
    }
    save(&dir.join("input.json"), &serde_json::to_value(input)?)?;
    let mut state = json!({"revision":revision,"nodes":{"COMPONENT_CONTRACTS":"running","PHOD_SPEC":"waiting","VERIFICATION":"waiting","CONSISTENCY_JOIN":"waiting"},"instances":[],"error":null});
    write_state(&dir, &state)?;
    let outcome = tail_steps(root, &dir, revision, input, &endpoint, &model, &mut state);
    if let Err(error) = &outcome {
        for node in TAIL_NODES {
            let current = state["nodes"][node].as_str().unwrap_or("waiting");
            state["nodes"][node] = json!(match current {
                "running" => "failed",
                "waiting" => "blocked",
                other => other,
            });
        }
        state["error"] = json!(error.to_string());
    }
    write_state(&dir, &state)?;
    outcome
}

fn tail_steps(
    root: &Path,
    dir: &Path,
    revision: u32,
    input: &TailInput,
    endpoint: &str,
    model: &str,
    state: &mut Value,
) -> Result<Value> {
    let content = ask(
        root,
        endpoint,
        model,
        &contracts_prompt(
            &input.idea,
            &input.architecture,
            &input.slice_index,
            "the hardening loop's answers",
        ),
        &dir.join("evidence/contracts"),
    )?;
    let contracts = parse_contracts(&content)?;
    let mut names = std::collections::BTreeSet::new();
    for (i, c) in contracts.iter().enumerate() {
        if c.name.trim().is_empty()
            || !names.insert(&c.name)
            || c.purpose.trim().is_empty()
            || c.behaviour.is_empty()
        {
            return Err(invalid("invalid or duplicate component contract"));
        }
        save(
            &dir.join(format!("contracts/component-{i}.json")),
            &json!({"revision":revision,"contract":c,"inputs":{"idea_sha256":input.idea_sha256,"architecture_sha256":input.architecture_sha256,"slice_index_sha256":input.slice_index_sha256}}),
        )?;
    }
    state["nodes"]["COMPONENT_CONTRACTS"] = json!("produced");
    state["nodes"]["PHOD_SPEC"] = json!("running");
    write_state(dir, state)?;
    // Fan-out: one spec per contract; each needs only its own contract.
    let mut instances = Vec::new();
    let mut specs = Vec::new();
    let mut unsupported = Vec::new();
    for (i, c) in contracts.iter().enumerate() {
        match spec(c) {
            Ok(s) => {
                save(&dir.join(format!("specs/component-{i}.json")), &s)?;
                specs.push(json!({"index":i,"name":c.name,"spec":s}));
                instances.push(
                    json!({"index":i,"name":c.name,"spec":"produced","verification":"waiting"}),
                );
            }
            Err(reason) => {
                unsupported.push(
                    json!({"index":i,"name":c.name,"status":format!("unsupported: {reason}")}),
                );
                instances.push(json!({"index":i,"name":c.name,"spec":format!("unsupported: {reason}"),"verification":"not applicable"}));
            }
        }
    }
    state["instances"] = json!(instances);
    state["nodes"]["PHOD_SPEC"] = json!("produced");
    state["nodes"]["VERIFICATION"] = json!("running");
    write_state(dir, state)?;
    // Fan-out: one verification per spec; each needs only its own spec.
    let mut verifications = Vec::new();
    for entry in &specs {
        let i = entry["index"].as_u64().unwrap_or(0) as usize;
        let v = verify(&contracts[i], &entry["spec"]);
        save(&dir.join(format!("verifications/component-{i}.json")), &v)?;
        state["instances"][i]["verification"] = v["status"].clone();
        verifications.push(json!({"index":i,"name":contracts[i].name,"verification":v}));
    }
    state["nodes"]["VERIFICATION"] = json!("produced");
    state["nodes"]["CONSISTENCY_JOIN"] = json!("running");
    write_state(dir, state)?;
    // The one join: every spec and verification.
    let prompt = format!(
        "Give PHOD readiness advice on these Component contracts, generated spec outcomes and their verifications. Advisory only; never approve a gate, rewrite contracts, or claim validation has run beyond what is listed. State unsupported limits plainly. Return concise plain text. {}",
        json!({"contracts":contracts,"specs":specs,"unsupported":unsupported,"verifications":verifications})
    );
    let advice = ask(root, endpoint, model, &prompt, &dir.join("evidence/join"))?;
    let verified = verifications
        .iter()
        .filter(|v| v["verification"]["status"] == "verified")
        .count();
    let join = json!({"revision":revision,"advisory":true,"endpoint":endpoint,"model":model,"content":advice,"contracts":contracts.len(),"specs":specs.len(),"unsupported":unsupported.len(),"verified":verified,"standing":"draft; no gate approval"});
    save(&dir.join("join.json"), &join)?;
    state["nodes"]["CONSISTENCY_JOIN"] = json!("produced");
    Ok(
        json!({"revision":revision,"contracts":contracts,"specs":specs,"unsupported":unsupported,"verifications":verifications,"join":join}),
    )
}

pub(crate) fn latest_revision(base: &Path) -> Result<Option<std::path::PathBuf>> {
    let entries = match fs::read_dir(base) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut best: Option<(u32, std::path::PathBuf)> = None;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(n) = name
            .to_str()
            .and_then(|n| n.strip_prefix('r'))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| n > *b) {
            best = Some((n, entry.path()));
        }
    }
    Ok(best.map(|(_, p)| p))
}

/// The tail's node states for the graph export, read from the latest revision; never starts anything.
pub fn tail_state(root: &Path, project: &str, branch: &str) -> Result<Value> {
    let Some(dir) = latest_revision(&tail_base(root, project, branch)?)? else {
        return Ok(
            json!({"revision":null,"nodes":{"COMPONENT_CONTRACTS":"waiting","PHOD_SPEC":"waiting","VERIFICATION":"waiting","CONSISTENCY_JOIN":"waiting"},"instances":[],"error":null}),
        );
    };
    match fs::read(dir.join("state.json")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        // A revision folder whose first state is not written yet is starting.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(
            json!({"revision":null,"nodes":{"COMPONENT_CONTRACTS":"running","PHOD_SPEC":"waiting","VERIFICATION":"waiting","CONSISTENCY_JOIN":"waiting"},"instances":[],"error":null}),
        ),
        Err(e) => Err(e.into()),
    }
}

fn readable_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    let cleaned: String = cleaned.chars().take(120).collect();
    if cleaned.is_empty() {
        "Component".into()
    } else {
        cleaned
    }
}

/// Readable files of the latest complete tail, as `(path under the project folder, text)`:
/// `product/contracts/<name>.md`, `product/specs/<name>.md` and `product/Consistency join.md`.
pub fn readable(
    root: &Path,
    project: &str,
    branch: &str,
    suffix: &str,
) -> Result<Vec<(String, String)>> {
    let Some(dir) = latest_revision(&tail_base(root, project, branch)?)? else {
        return Ok(Vec::new());
    };
    let mut files = Vec::new();
    let mut i = 0;
    while let Ok(bytes) = fs::read(dir.join(format!("contracts/component-{i}.json"))) {
        let record: Value = serde_json::from_slice(&bytes)?;
        let c: Contract = serde_json::from_value(record["contract"].clone())?;
        let name = format!("{}{suffix}", readable_name(&c.name));
        let list = |items: &[String]| {
            if items.is_empty() {
                "- none\n".to_string()
            } else {
                items.iter().map(|s| format!("- {s}\n")).collect()
            }
        };
        files.push((
            format!("product/contracts/{name}.md"),
            format!(
                "# {}\n\n{}\n\n## Inputs\n{}\n## Outputs\n{}\n## Behaviour\n{}\n## Errors\n{}\n## Effects\n{}",
                c.name,
                c.purpose,
                list(&c.inputs),
                list(&c.outputs),
                list(&c.behaviour),
                list(&c.errors),
                list(&c.effects)
            ),
        ));
        let spec_text = match fs::read(dir.join(format!("specs/component-{i}.json"))) {
            Ok(spec) => {
                let verification = fs::read(dir.join(format!("verifications/component-{i}.json")))
                    .ok()
                    .and_then(|v| serde_json::from_slice::<Value>(&v).ok());
                format!(
                    "# {} — PHOD spec\n\nProfile qroc-record/1.\n\n```json\n{}\n```\n\nVerification: {}\n",
                    c.name,
                    String::from_utf8_lossy(&spec),
                    verification
                        .as_ref()
                        .and_then(|v| v["status"].as_str())
                        .unwrap_or("waiting")
                )
            }
            Err(_) => format!(
                "# {} — PHOD spec\n\nUnsupported: {}\n",
                c.name,
                spec(&c).err().unwrap_or_else(|| "not produced yet".into())
            ),
        };
        files.push((format!("product/specs/{name}.md"), spec_text));
        i += 1;
    }
    if let Ok(bytes) = fs::read(dir.join("join.json")) {
        let join: Value = serde_json::from_slice(&bytes)?;
        files.push((
            format!("product/Consistency join{suffix}.md"),
            format!(
                "# Consistency join\n\nContracts {}; specs {}; unsupported {}; verified {} of {}.\n{}\n{}\n",
                join["contracts"],
                join["specs"],
                join["unsupported"],
                join["verified"],
                join["specs"],
                if join["source"] == "work-table" {
                    format!(
                        "From the work table {}: joined {}, missing {}, failed {}.\n",
                        join["run"].as_str().unwrap_or(""),
                        join["joined"],
                        join["missing"],
                        join["failed"]
                    )
                } else {
                    String::new()
                },
                join["content"].as_str().unwrap_or("")
            ),
        ));
    }
    Ok(files)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn contract() -> Contract {
        let r = RecordShape {
            text_field: "title".into(),
            integer_field: "slides".into(),
            minimum: 1,
            maximum: 100,
            unknown_fields: "reject".into(),
        };
        Contract {
            name: "Scene metadata".into(),
            purpose: "Inspect metadata".into(),
            inputs: vec!["record".into()],
            outputs: vec!["validity".into()],
            behaviour: record_behaviour(&r),
            errors: vec!["reject invalid record".into()],
            effects: vec![],
            profile: "qroc-record/1".into(),
            record: Some(r),
            unsupported_reason: None,
        }
    }
    #[test]
    fn supported_preserves_all_clauses() {
        let c = contract();
        let s = spec(&c).unwrap();
        assert_eq!(s["minimum"], 1);
        assert_eq!(s["maximum"], 100);
        assert_eq!(s["unknown_fields"], "reject");
        assert_eq!(s["text_field"], "title");
    }
    #[test]
    fn effects_and_extra_behaviour_are_unsupported() {
        let mut c = contract();
        c.effects.push("write deck".into());
        assert!(spec(&c).is_err());
        c.effects.clear();
        c.behaviour.push("render slides".into());
        assert!(spec(&c).is_err());
    }
    #[test]
    fn invalid_shapes_are_unsupported() {
        let mut c = contract();
        c.record.as_mut().unwrap().maximum = 0;
        assert!(spec(&c).is_err());
        c.record.as_mut().unwrap().maximum = 100;
        c.record.as_mut().unwrap().unknown_fields = "guess".into();
        assert!(spec(&c).is_err());
    }
    #[test]
    fn unsupported_never_gets_a_spec() {
        let mut c = contract();
        c.profile = "deck".into();
        c.unsupported_reason = Some("deck rendering is outside the pure record profile".into());
        assert!(spec(&c).unwrap_err().contains("deck rendering"));
    }
}
