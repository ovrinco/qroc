//! The wide stage handed to QREAM: the per-contract PHOD spec and verification fan-out is
//! written as a work table (FORGE.md columns) and its results are read back into the
//! Consistency join. Q-ROC plans and joins; it starts nothing and schedules nothing.
use crate::contracts::{self, Contract};
use crate::{Error, Result};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Charter OC-09, result line 8, copied from OUTCOME.md.
pub const OUTCOME_LINE: &str = "Hand the stage where many independent pieces are produced side by side to QREAM as a work table, and get the results back where they join (the first half of the goal-to-build demo)";
/// The work table's `outcome` cell (forge allows `[A-Za-z0-9_-]` only).
pub const OUTCOME_CELL: &str = "OC-09-line-8";
pub const RESULT_SCHEMA: &str = "qroc-worktable-result/1";
pub const COLUMNS: [&str; 12] = [
    "id",
    "subject",
    "department",
    "kind",
    "model",
    "effort",
    "cwd",
    "input_slice",
    "writable_scope",
    "acceptance_commands",
    "outcome",
    "account",
];

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn identity<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    let s = args[key].as_str().unwrap_or("");
    if s.is_empty()
        || s.len() > 200
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(invalid(format!("Invalid {key}")));
    }
    Ok(s)
}

/// The project folder (`<root>/project-folders/<title>`), claimed for this project only.
fn project_folder(root: &Path, project: &str, branch: &str) -> Result<PathBuf> {
    let record: Value = serde_json::from_slice(&fs::read(
        root.join("projects").join(format!("{project}.json")),
    )?)?;
    let owner: Value = serde_json::from_slice(&fs::read(
        root.join("branches").join(format!("{branch}.json")),
    )?)?;
    if record["id"] != project || owner["id"] != branch || owner["project_id"] != project {
        return Err(invalid("Work table project or branch mismatch"));
    }
    let title = record["title"].as_str().unwrap_or("");
    if title.trim().is_empty()
        || title.starts_with('.')
        || title
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(invalid("Project title cannot name a folder"));
    }
    let folder = root.join("project-folders").join(title);
    crate::scope::validate_root(&folder)?;
    let marker = folder.join(".qroc-owner");
    if folder.exists() {
        if fs::read_to_string(&marker).ok().as_deref() != Some(project) {
            return Err(invalid("Project folder is owned by another project"));
        }
    } else {
        fs::create_dir_all(&folder)?;
        fs::write(&marker, project)?;
    }
    Ok(folder)
}

fn cell(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out: String = out.trim_matches('_').chars().take(30).collect();
    let out = out.trim_end_matches('_');
    if out.is_empty() {
        "component".into()
    } else {
        out.into()
    }
}

/// Directive template for one row; `{{column}}` fields are filled by the forge.
pub fn template(run_dir: &Path) -> String {
    let root = run_dir.display();
    format!(
        r#"# {{{{id}}}} — PHOD spec and verification for one Component contract

## Orientation
Product: Q-ROC, the Product tail's wide stage (one PHOD spec and its verification per Component contract).
Outcome: OC-09, result line 8: {OUTCOME_LINE}.
Posture: apply.
This row is {{{{id}}}} ({{{{subject}}}}). It needs only its own contract; it starts no model, no build and nothing on the GPU.

## Inputs
- {{{{input_slice}}}} (read only): the contract record; `contract` holds the Component contract.

## Writable scope
- {{{{writable_scope}}}}: write `result.json` there.
- {root}/reports/{{{{id}}}}_{{{{subject}}}}.md: the report. Nothing else.

## Work items
1. Spec. The only supported profile is `qroc-record/1`: pure inspection of one record with a nonempty text field and
   a bounded integer field, plus an unknown-field policy. It applies only when `profile` is `qroc-record/1`, `effects`
   is empty, `record` is present with distinct nonempty `text_field` and `integer_field`, `minimum` <= `maximum`,
   `unknown_fields` one of reject, preserve, ignore, `behaviour` is exactly
   `["nonempty text: TEXT", "integer: INT in MIN..MAX", "unknown fields: POLICY"]` and `errors` is exactly
   `["reject invalid record"]`. Then the spec is the object
   `{{"profile":"qroc-record/1","name":NAME,"text_field":..,"integer_field":..,"minimum":..,"maximum":..,"unknown_fields":..}}`.
   Otherwise the contract is unsupported: say why in plain words (its own `unsupported_reason` when it gives one).
2. Verification. Derive the spec a second time from the contract and compare: identical is `verified`, else `failed`
   with the reason.
3. Write `result.json` in {{{{writable_scope}}}}:
   `{{"schema":"{RESULT_SCHEMA}","row":"{{{{id}}}}","contract_sha256":<contract_sha256 from the input>,"outcome":"spec"|"unsupported","spec":<object or null>,"unsupported_reason":<text or null>,"verification":{{"status":"verified"|"failed","reason":<text or null>}} or null}}`.

## Shared resources
GPU: none. Seat: none. Ports: none. Provider: none. CPU: none beyond reading and writing one file.

## Acceptance checklist
1. `{{{{acceptance_commands}}}}` exits 0.
2. `result.json` names row {{{{id}}}} and the input's contract_sha256.
3. `python3 -c "import json,sys; r=json.load(open(sys.argv[1])); sys.exit(r['schema']!='{RESULT_SCHEMA}')" '{{{{writable_scope}}}}/result.json'` exits 0.

## Output
`result.json` in {{{{writable_scope}}}}. Report: {root}/reports/{{{{id}}}}_{{{{subject}}}}.md, naming the outcome and the
checklist exits, then:
Surprise:
Friction:
Directive fix:
Look hardest at:

Seal: `python3 ~/.claude/skills/swarm-campaign/scripts/seal.py {root} {{{{id}}}}`.

## Blockers
Report once (E9) with the command, its output and a continuation prompt, then idle. Never alter the contract.
"#
    )
}

/// Options for the work table's policy columns. Accounts are plain labels.
pub struct ExportOptions {
    pub account: String,
    pub effort: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            account: "codex".into(),
            effort: "medium".into(),
        }
    }
}

fn read_contracts(dir: &Path) -> Result<Vec<(Value, Contract)>> {
    let mut out = Vec::new();
    while let Ok(bytes) = fs::read(dir.join(format!("contracts/component-{}.json", out.len()))) {
        let record: Value = serde_json::from_slice(&bytes)?;
        let contract: Contract = serde_json::from_value(record["contract"].clone())?;
        out.push((record, contract));
    }
    Ok(out)
}

/// Writes `work-table/<run>/table.tsv`, `template.md`, `manifest.json` and one row folder
/// (`rows/Tnnn/contract.json`, the worker's cwd and private scope) per contract of the
/// latest tail revision. Nothing is started.
pub fn export(root: &Path, project: &str, branch: &str, options: &ExportOptions) -> Result<Value> {
    crate::scope::validate_root(root)?;
    if !["low", "medium"].contains(&options.effort.as_str())
        || options.account.trim().is_empty()
        || options
            .account
            .chars()
            .any(|c| c.is_control() || c == '|' || c == '{')
    {
        return Err(invalid(
            "Effort must be low or medium; account a plain label",
        ));
    }
    let base = contracts::tail_base(root, project, branch)?;
    let tail = contracts::latest_revision(&base)?
        .ok_or_else(|| invalid("No Component contracts exist for this branch yet"))?;
    let revision: u64 = tail
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix('r'))
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| invalid("Unreadable tail revision"))?;
    let found = read_contracts(&tail)?;
    if found.is_empty() {
        return Err(invalid("No Component contracts exist for this branch yet"));
    }
    if found.len() > 999 {
        return Err(invalid("At most 999 rows fit one work table"));
    }
    // The run's working root is the tail store (identities only, no spaces: QREAM's lint reads
    // the seal line's root as one word); the project folder gets readable copies.
    let readable = project_folder(root, project, branch)?.join("work-table");
    let tables = base.join("work-table");
    fs::create_dir_all(&tables)?;
    let mut n = 1;
    let (run, dir) = loop {
        let run = format!("{branch}-r{revision}-w{n}");
        let d = tables.join(&run);
        match fs::create_dir(&d) {
            Ok(()) => break (run, d),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e.into()),
        }
    };
    let mut tsv = COLUMNS.join("\t") + "\n";
    let mut rows = Vec::new();
    for (i, (record, contract)) in found.iter().enumerate() {
        let id = format!("T{:03}", i + 1);
        let subject = format!("spec_{}", slug(&contract.name));
        let row_dir = dir.join("rows").join(&id);
        fs::create_dir_all(&row_dir)?;
        let contract_sha256 = contracts::hash(&serde_json::to_vec(&record["contract"])?);
        let input = row_dir.join("contract.json");
        contracts::save(
            &input,
            &json!({"row":id,"index":i,"tail_revision":revision,"contract_sha256":contract_sha256,"contract":record["contract"]}),
        )?;
        let result = row_dir.join("result.json");
        let cells = [
            id.clone(),
            subject.clone(),
            "qroc-specs".into(),
            "codex".into(),
            "gpt-6.1-sol".into(),
            options.effort.clone(),
            row_dir.display().to_string(),
            input.display().to_string(),
            row_dir.display().to_string(),
            format!("python3 -m json.tool '{}'", result.display()),
            OUTCOME_CELL.into(),
            options.account.clone(),
        ];
        tsv += &cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join("\t");
        tsv.push('\n');
        rows.push(json!({"row":id,"subject":subject,"index":i,"name":contract.name,"contract_sha256":contract_sha256}));
    }
    fs::write(dir.join("table.tsv"), &tsv)?;
    fs::create_dir_all(dir.join("reports"))?;
    fs::write(dir.join("template.md"), template(&dir))?;
    let manifest = json!({"run":run,"project_id":project,"branch_id":branch,"tail_revision":revision,"outcome":format!("OC-09, result line 8: {OUTCOME_LINE}"),"result_schema":RESULT_SCHEMA,"rows":rows,"started":false});
    contracts::save(&dir.join("manifest.json"), &manifest)?;
    let shown = readable.join(&run);
    fs::create_dir_all(&shown)?;
    fs::write(shown.join("table.tsv"), &tsv)?;
    fs::write(shown.join("template.md"), template(&dir))?;
    Ok(
        json!({"run":run,"folder":dir.display().to_string(),"table":dir.join("table.tsv").display().to_string(),"template":dir.join("template.md").display().to_string(),"rows":rows.len(),"started":false}),
    )
}

/// One row as it comes back: `joined` (a verified spec, or a confirmed unsupported
/// reason), `failed` (with the reason) or `missing` (no result file).
fn judge(row: &Value, contract: &Contract, result: Option<&[u8]>) -> Value {
    let id = row["row"].as_str().unwrap_or("");
    let name = &contract.name;
    let Some(bytes) = result else {
        return json!({"row":id,"name":name,"status":"missing","reason":"no result returned"});
    };
    let fail = |reason: String| json!({"row":id,"name":name,"status":"failed","reason":reason});
    let r: Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => return fail(format!("result is not JSON: {e}")),
    };
    if r["schema"] != RESULT_SCHEMA || r["row"] != id {
        return fail("result names another schema or row".into());
    }
    if r["contract_sha256"] != row["contract_sha256"] {
        return fail("result was made from another contract".into());
    }
    match r["outcome"].as_str() {
        Some("spec") => {
            if r["verification"]["status"] != "verified" {
                return fail(format!(
                    "the worker's verification failed: {}",
                    r["verification"]["reason"]
                        .as_str()
                        .unwrap_or("no reason given")
                ));
            }
            let check = contracts::verify(contract, &r["spec"]);
            if check["status"] != "verified" {
                return fail(format!(
                    "the spec does not verify against its contract: {}",
                    check["reason"].as_str().unwrap_or("")
                ));
            }
            json!({"row":id,"name":name,"status":"joined","spec":r["spec"],"verification":check})
        }
        Some("unsupported") => match contracts::spec(contract) {
            Ok(_) => fail("the worker called a supported contract unsupported".into()),
            Err(reason) => {
                json!({"row":id,"name":name,"status":"joined","unsupported":format!("unsupported: {reason}"),"worker_reason":r["unsupported_reason"]})
            }
        },
        _ => fail("result outcome is neither spec nor unsupported".into()),
    }
}

/// Reads one result per row and writes the join as a new tail revision, as if the tail had
/// produced it: contracts, specs, verifications, state and `join.json`. Missing and failed rows
/// stay visible. With `advice`, the configured route is asked for readiness advice as the
/// tail's own join does; without it, no provider is asked.
pub fn join(root: &Path, project: &str, branch: &str, run: &str, advice: bool) -> Result<Value> {
    crate::scope::validate_root(root)?;
    identity(&json!({"run":run}), "run")?;
    let readable = project_folder(root, project, branch)?
        .join("work-table")
        .join(run);
    let dir = contracts::tail_base(root, project, branch)?
        .join("work-table")
        .join(run);
    let manifest: Value = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?;
    if manifest["project_id"] != project || manifest["branch_id"] != branch {
        return Err(invalid("Work table belongs to another project or branch"));
    }
    let rows = manifest["rows"]
        .as_array()
        .ok_or_else(|| invalid("Work table manifest has no rows"))?;
    let mut contracts_list = Vec::new();
    let mut judged = Vec::new();
    for row in rows {
        let id = identity(row, "row")?;
        let input: Value =
            serde_json::from_slice(&fs::read(dir.join("rows").join(id).join("contract.json"))?)?;
        let contract: Contract = serde_json::from_value(input["contract"].clone())?;
        if contracts::hash(&serde_json::to_vec(&input["contract"])?) != row["contract_sha256"] {
            return Err(invalid("A row's contract changed after export"));
        }
        let result = match fs::read(dir.join("rows").join(id).join("result.json")) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        judged.push(judge(row, &contract, result.as_deref()));
        contracts_list.push(contract);
    }
    let count = |s: &str| judged.iter().filter(|j| j["status"] == s).count();
    let (joined, missing, failed) = (count("joined"), count("missing"), count("failed"));
    // A new tail revision; earlier ones are kept.
    let base = contracts::tail_base(root, project, branch)?;
    fs::create_dir_all(&base)?;
    let mut revision = 1u32;
    let out = loop {
        let d = base.join(format!("r{revision}"));
        match fs::create_dir(&d) {
            Ok(()) => break d,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => revision += 1,
            Err(e) => return Err(e.into()),
        }
    };
    for folder in ["contracts", "specs", "verifications", "evidence"] {
        fs::create_dir(out.join(folder))?;
    }
    let mut instances = Vec::new();
    let mut specs = Vec::new();
    let mut unsupported = Vec::new();
    let mut verifications = Vec::new();
    for (i, (c, j)) in contracts_list.iter().zip(&judged).enumerate() {
        contracts::save(
            &out.join(format!("contracts/component-{i}.json")),
            &json!({"revision":revision,"contract":c,"inputs":{"work_table":run,"tail_revision":manifest["tail_revision"]}}),
        )?;
        let status = j["status"].as_str().unwrap_or("failed");
        if status != "joined" {
            instances.push(json!({"index":i,"name":c.name,"row":j["row"],"spec":status,"verification":status,"reason":j["reason"]}));
        } else if let Some(reason) = j["unsupported"].as_str() {
            unsupported.push(json!({"index":i,"name":c.name,"status":reason}));
            instances.push(json!({"index":i,"name":c.name,"row":j["row"],"spec":reason,"verification":"not applicable"}));
        } else {
            contracts::save(&out.join(format!("specs/component-{i}.json")), &j["spec"])?;
            contracts::save(
                &out.join(format!("verifications/component-{i}.json")),
                &j["verification"],
            )?;
            specs.push(json!({"index":i,"name":c.name,"spec":j["spec"]}));
            verifications.push(json!({"index":i,"name":c.name,"verification":j["verification"]}));
            instances.push(json!({"index":i,"name":c.name,"row":j["row"],"spec":"produced","verification":"verified"}));
        }
    }
    contracts::save(
        &out.join("evidence/work-table.json"),
        &json!({"run":run,"rows":judged}),
    )?;
    let fan = if joined == rows.len() {
        "produced"
    } else {
        "failed"
    };
    let mut state = json!({"revision":revision,"source":"work-table","run":run,"nodes":{"COMPONENT_CONTRACTS":"produced","PHOD_SPEC":fan,"VERIFICATION":fan,"CONSISTENCY_JOIN":"running"},"instances":instances,"error":null});
    contracts::write_state(&out, &state)?;
    let verified = verifications.len();
    let mut content = format!(
        "Joined {joined} of {} rows from the work table {run}; missing {missing}; failed {failed}.",
        rows.len()
    );
    for j in judged.iter().filter(|j| j["status"] != "joined") {
        content += &format!(
            "\n- {} ({}): {}, {}",
            j["name"].as_str().unwrap_or(""),
            j["row"].as_str().unwrap_or(""),
            j["status"].as_str().unwrap_or(""),
            j["reason"].as_str().unwrap_or("")
        );
    }
    let mut join = json!({"revision":revision,"source":"work-table","run":run,"advisory":true,"contracts":contracts_list.len(),"specs":specs.len(),"unsupported":unsupported.len(),"verified":verified,"joined":joined,"missing":missing,"failed":failed,"rows":judged,"standing":"draft; no gate approval"});
    if advice {
        let asked = (|| -> Result<Value> {
            let (endpoint, model) = crate::registry::provider_config_at(root)?;
            let prompt = format!(
                "Give PHOD readiness advice on these Component contracts, generated spec outcomes and their verifications, returned from a work table. Rows listed as missing or failed produced nothing usable; say so plainly. Advisory only; never approve a gate, rewrite contracts, or claim validation has run beyond what is listed. Return concise plain text. {}",
                json!({"contracts":contracts_list,"specs":specs,"unsupported":unsupported,"verifications":verifications,"rows":judged})
            );
            let text =
                contracts::ask(root, &endpoint, &model, &prompt, &out.join("evidence/join"))?;
            Ok(json!({"endpoint":endpoint,"model":model,"text":text}))
        })();
        match asked {
            Ok(a) => {
                content += &format!("\n\n{}", a["text"].as_str().unwrap_or(""));
                join["endpoint"] = a["endpoint"].clone();
                join["model"] = a["model"].clone();
            }
            Err(e) => {
                state["nodes"]["CONSISTENCY_JOIN"] = json!("failed");
                state["error"] = json!(e.to_string());
                contracts::write_state(&out, &state)?;
                return Err(e);
            }
        }
    }
    join["content"] = json!(content);
    contracts::save(&out.join("join.json"), &join)?;
    fs::create_dir_all(&readable)?;
    fs::write(
        readable.join(format!("Join revision {revision}.md")),
        format!("# Work table {run}: joined back\n\n{content}\n"),
    )?;
    state["nodes"]["CONSISTENCY_JOIN"] = json!("produced");
    contracts::write_state(&out, &state)?;
    Ok(join)
}

/// Bridge commands `qroc-worktable-export` and `qroc-worktable-join`.
pub fn command(root: &Path, command: &str, args: &Value) -> Result<Value> {
    let project = identity(args, "project_id")?;
    let branch = identity(args, "branch_id")?;
    match command {
        "qroc-worktable-export" => {
            let mut options = ExportOptions::default();
            if let Some(a) = args["account"].as_str() {
                options.account = a.into();
            }
            if let Some(e) = args["effort"].as_str() {
                options.effort = e.into();
            }
            export(root, project, branch, &options)
        }
        "qroc-worktable-join" => join(
            root,
            project,
            branch,
            identity(args, "run")?,
            args["advice"].as_bool().unwrap_or(false),
        ),
        _ => Err(invalid(
            "capability-unavailable: work-table command is not implemented",
        )),
    }
}
