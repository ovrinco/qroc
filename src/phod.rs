//! Bounded durable adapter for the real Phod parser/compiler toolchain.
//!
//! The desktop owns drafts and source archives. Validation, compilation,
//! verification, and charter work are delegated to explicitly discovered Phod
//! tools; this module never substitutes a local heuristic for those tools.
//! When Zig or a configured tool is absent, the job is durably marked
//! `unavailable` with the exact missing capability.

use crate::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const MAX_SOURCE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 32 * 1024 * 1024;
const MAX_TOOL_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_ACTIVE_JOBS: usize = 4;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn stamp() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos}-{}", std::process::id())
}

fn arg_string(args: &Value, key: &str, max: usize) -> Result<String> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("phod requires string {key}")))?;
    if value.is_empty() || value.len() > max {
        return Err(invalid(format!("phod {key} is empty or too long")));
    }
    Ok(value.to_owned())
}

fn safe_component(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(invalid(format!("invalid {label}")));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("Phod symlink inputs are refused"));
    }
    Ok(())
}

fn reject_symlink_components(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(invalid("Phod symlink path components are refused"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn regular_source(path: &Path) -> Result<()> {
    reject_symlink_components(path)?;
    reject_symlink(path)?;
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid("Phod source must be a regular file"));
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(invalid("Phod source exceeds the 10 MiB limit"));
    }
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    regular_source(path)?;
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(invalid("Phod source exceeds the 10 MiB limit"));
    }
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(invalid("Phod generated record exceeds the 32 MiB limit"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Phod output has no parent"))?;
    reject_symlink_components(parent)?;
    match fs::symlink_metadata(path) {
        Ok(_) => reject_symlink(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(parent)?;
    let mut temp = path.to_path_buf();
    temp.set_extension(format!("tmp-{}", stamp()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    let dir = File::open(parent)?;
    dir.sync_all()?;
    Ok(())
}

fn copy_archive(source: &Path, destination: &Path) -> Result<(u64, String)> {
    let bytes = read_bounded(source)?;
    let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
    atomic_write(destination, &bytes)?;
    Ok((bytes.len() as u64, hash))
}

fn draft_text(args: &Value) -> Result<String> {
    let name = arg_string(args, "intent_name", 160)
        .or_else(|_| arg_string(args.get("intent").unwrap_or(&Value::Null), "name", 160))?;
    let purpose = arg_string(args, "purpose", MAX_TEXT_BYTES).or_else(|_| {
        arg_string(
            args.get("intent").unwrap_or(&Value::Null),
            "purpose",
            MAX_TEXT_BYTES,
        )
    })?;
    let constraints = args
        .get("constraints")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("phod draft requires constraints array"))?;
    let mut normalized = Vec::new();
    let mut has_language = false;
    let mut checks = Vec::new();
    for item in constraints {
        let value = item
            .as_str()
            .ok_or_else(|| invalid("phod constraints must be strings"))?;
        if value.len() > 4096 {
            return Err(invalid("phod constraint is too long"));
        }
        let (key, raw_value) = value.split_once(':').ok_or_else(|| {
            invalid(format!(
                "phod constraint must use namespace: value syntax: {value}"
            ))
        })?;
        let key = key.trim();
        let raw_value = raw_value.trim();
        if key.is_empty() || raw_value.is_empty() || !key.contains('.') {
            return Err(invalid(format!(
                "invalid Phod constraint namespace: {value}"
            )));
        }
        if key == "target.language" {
            has_language = true;
        }
        if key == "verification.required_checks" {
            checks.extend(
                raw_value
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned),
            );
        } else {
            normalized.push((key.to_owned(), raw_value.to_owned()));
        }
    }
    if !has_language {
        normalized.insert(0, ("target.language".to_owned(), "rust".to_owned()));
    }
    if checks.is_empty() {
        checks.push("compile".to_owned());
    }
    let mut text = format!("===INTENT===\nname: {name}\npurpose: {purpose}\n\n===CONSTRAINTS===\n");
    for (key, value) in normalized {
        text.push_str(&key);
        text.push_str(": ");
        text.push_str(&value);
        text.push('\n');
    }
    text.push_str("verification.required_checks:\n");
    for check in checks {
        text.push_str("  - ");
        text.push_str(&check);
        text.push('\n');
    }
    text.push_str("\n===DERIVED===\n");
    Ok(text)
}

#[derive(Clone)]
struct Job {
    value: Value,
}

/// The stop flag and child process ids of one running job.
type JobControl = (Arc<AtomicBool>, Arc<Mutex<BTreeSet<u32>>>);

pub struct PhodStore {
    root: PathBuf,
    jobs: Arc<Mutex<BTreeMap<String, Job>>>,
    children: Arc<Mutex<BTreeSet<u32>>>,
    tooling_cache: Value,
    controls: BTreeMap<String, JobControl>,
}

fn stop_group(pid: u32) {
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
impl Drop for PhodStore {
    fn drop(&mut self) {
        let ids = self
            .children
            .lock()
            .map(|ids| ids.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for id in ids {
            stop_group(id);
        }
        for (cancel, children) in self.controls.values() {
            cancel.store(true, Ordering::SeqCst);
            if let Ok(ids) = children.lock() {
                for id in ids.iter() {
                    stop_group(*id);
                }
            }
        }
    }
}
struct ToolGuard {
    pid: u32,
    children: Arc<Mutex<BTreeSet<u32>>>,
}
impl Drop for ToolGuard {
    fn drop(&mut self) {
        stop_group(self.pid);
        if let Ok(mut children) = self.children.lock() {
            children.remove(&self.pid);
        }
    }
}
fn rustc_path() -> Option<PathBuf> {
    for key in ["SOLITON_RUSTC", "RUSTC"] {
        if let Some(path) = env::var_os(key) {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    if let Some(paths) = env::var_os("PATH")
        && let Some(path) = env::split_paths(&paths)
            .map(|p| p.join("rustc"))
            .find(|p| p.is_file())
    {
        return Some(path);
    }
    None
}
fn spawn_tool(
    tool: &Path,
    input: &Path,
    cwd: &Path,
    children: &Arc<Mutex<BTreeSet<u32>>>,
) -> Result<(Child, ToolGuard)> {
    let mut command = Command::new(tool);
    command
        .arg(input)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = rustc_path() {
        command.env("RUSTC", path);
    }
    if let Some(root) = cwd.parent().and_then(Path::parent)
        && let Some(id) = cwd.file_name()
    {
        command.env("PHOD_OWNER_DIRECTORY", root.join("admissions").join(id));
    }
    #[cfg(unix)]
    command.process_group(0);
    let child = command.spawn()?;
    let pid = child.id();
    children
        .lock()
        .map_err(|_| invalid("Phod process registry lock failed"))?
        .insert(pid);
    Ok((
        child,
        ToolGuard {
            pid,
            children: Arc::clone(children),
        },
    ))
}

impl PhodStore {
    pub fn open(root: &Path) -> Result<Self> {
        match fs::symlink_metadata(root) {
            Ok(_) => reject_symlink(root)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        reject_symlink_components(root)?;
        fs::create_dir_all(root)?;
        reject_symlink(root)?;
        for name in ["drafts", "archives", "jobs", "outputs", "admissions"] {
            let path = root.join(name);
            reject_symlink_components(&path)?;
            fs::create_dir_all(&path)?;
            reject_symlink(&path)?;
        }
        let mut records = BTreeMap::new();
        for entry in fs::read_dir(root.join("jobs"))? {
            let path = entry?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let mut bytes = Vec::new();
            File::open(&path)?
                .take(MAX_TEXT_BYTES as u64)
                .read_to_end(&mut bytes)?;
            let mut value: Value = serde_json::from_slice(&bytes)?;
            if matches!(
                value.get("state").and_then(Value::as_str),
                Some("queued" | "running")
            ) {
                value["state"] = json!("interrupted");
                value["error"] = json!("Phod receiver restarted while the job was running");
                value["completed_at"] = json!(now());
                let mut items = Vec::new();
                if let Some(source) = value["source_path"].as_str()
                    && let Ok(item) = artifact(Path::new(source), "input")
                {
                    items.push(item);
                }
                if let Some(id) = value["id"].as_str() {
                    for name in ["outputs", "admissions"] {
                        let p = root.join(name).join(id);
                        if p.is_dir() {
                            collect_artifacts(&p, &mut items)?;
                        }
                    }
                }
                value["evidence"]["artifacts"] = json!(items);
                atomic_write(&path, serde_json::to_vec(&value)?.as_slice())?;
            }
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                records.insert(id.to_owned(), Job { value });
            }
        }
        Ok(Self {
            root: root.to_owned(),
            jobs: Arc::new(Mutex::new(records)),
            children: Arc::new(Mutex::new(BTreeSet::new())),
            tooling_cache: tooling(),
            controls: BTreeMap::new(),
        })
    }

    pub fn command(&mut self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "phod-list" => self.list(args),
            "phod-draft-save" => self.draft_save(args),
            "phod-workbench-spec-save" => self.workbench_spec_save(args),
            "phod-job-list" => self.list_jobs(args),
            "phod-job-status" => self.status(arg_string(args, "id", 180)?),
            "phod-artifact-read" => self.read_artifact(args),
            "phod-job-cancel" => self.cancel(args),
            "phod-replay" => self.replay(args),
            "phod-validate" | "phod-compile" | "phod-verify" | "phod-charter"
            | "phod-workbench" => self.start(command, args),
            _ => Err(invalid(format!("Unknown Phod command: {command}"))),
        }
    }

    fn list(&self, args: &Value) -> Result<Value> {
        let mut result = self.list_jobs(args)?;
        result["jobs"] = result["items"].take();
        result["tooling"] = self.tooling_cache.clone();
        result["capabilities"] = capabilities(&self.tooling_cache);
        Ok(result)
    }
    fn list_jobs(&self, args: &Value) -> Result<Value> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| invalid("Phod job lock poisoned"))?;
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 200) as usize;
        let items: Vec<_> = jobs
            .values()
            .rev()
            .skip(offset)
            .take(limit)
            .map(|job| {
                let mut value = job.value.clone();
                value.as_object_mut().unwrap().remove("evidence");
                value
            })
            .collect();
        Ok(json!({"items":items,"total":jobs.len(),"offset":offset,"limit":limit}))
    }

    fn status(&self, id: String) -> Result<Value> {
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| invalid("Phod job lock poisoned"))?;
        jobs.get(&id)
            .map(|job| job.value.clone())
            .ok_or_else(|| invalid("Phod job not found"))
    }

    fn read_artifact(&self, args: &Value) -> Result<Value> {
        let job = self.status(arg_string(args, "id", 180)?)?;
        let requested = arg_string(args, "path", 4096)?;
        let item = job["evidence"]["artifacts"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["path"].as_str() == Some(&requested))
            })
            .ok_or_else(|| invalid("Artifact is not recorded in this Phod job"))?;
        let path = Path::new(&requested);
        if !path.starts_with(&self.root) {
            return Err(invalid("Artifact is outside the owned Phod archive"));
        }
        let bytes = read_bounded(path)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if item["sha256"].as_str() != Some(&hash) {
            return Err(invalid(
                "Artifact changed since the recorded check; replay requires a new job",
            ));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| invalid("Artifact is binary, not a text input or output"))?;
        Ok(
            json!({"id":job["id"],"path":path,"sha256":hash,"role":item["role"],"text":text,"source_hash":job["source_hash"]}),
        )
    }

    fn workbench_spec_save(&self, args: &Value) -> Result<Value> {
        if !args.is_object() {
            return Err(invalid("Workbench specification must be an object"));
        }
        let bytes = serde_json::to_vec(args)?;
        if bytes.len() > 65536 {
            return Err(invalid("Workbench specification exceeds 64 KiB"));
        }
        let id = format!("phod-spec-{}", stamp());
        let path = self.root.join("archives").join(format!("{id}.json"));
        atomic_write(&path, &bytes)?;
        Ok(
            json!({"id":id,"source_path":path,"source_hash":format!("sha256:{:x}",Sha256::digest(&bytes)),"state":"saved"}),
        )
    }
    fn cancel(&mut self, args: &Value) -> Result<Value> {
        let id = arg_string(args, "id", 180)?;
        let record = self.status(id.clone())?;
        if !matches!(record["state"].as_str(), Some("queued" | "running")) {
            return Ok(record);
        }
        let (cancel, children) = self
            .controls
            .get(&id)
            .ok_or_else(|| invalid("Job has no owned process controller"))?;
        cancel.store(true, Ordering::SeqCst);
        for pid in children
            .lock()
            .map_err(|_| invalid("Process lock poisoned"))?
            .iter()
        {
            stop_group(*pid);
        }
        Ok(json!({"id":id,"state":"cancelling","artifacts_preserved":true}))
    }
    fn replay(&self, args: &Value) -> Result<Value> {
        let job = self.status(arg_string(args, "id", 180)?)?;
        if matches!(job["state"].as_str(), Some("queued" | "running")) {
            return Err(invalid("Wait for terminal disposition before replay"));
        }
        let artifacts = job["evidence"]["artifacts"]
            .as_array()
            .ok_or_else(|| invalid("No retained evidence to replay"))?;
        for item in artifacts {
            self.read_artifact(&json!({"id":job["id"],"path":item["path"]}))
                .or_else(|e| {
                    // Binary outputs are hash checked too; reading text is not necessary.
                    if e.to_string().contains("Artifact is binary") {
                        Ok(Value::Null)
                    } else {
                        Err(e)
                    }
                })?;
        }
        let mut decisions = Vec::new();
        if job["action"] == "workbench" {
            let report: Value = serde_json::from_str(
                job["evidence"]["stdout"]
                    .as_str()
                    .ok_or_else(|| invalid("Workbench receipt missing"))?,
            )?;
            let owner = self.root.join("admissions").join(
                job["id"]
                    .as_str()
                    .ok_or_else(|| invalid("Job identity missing"))?,
            );
            let binding: Value =
                serde_json::from_slice(&read_bounded(&owner.join("owner-binding.json"))?)?;
            let tool=PathBuf::from(binding["admitted_checker"]["path"].as_str().ok_or_else(||invalid("unavailable: owner-admitted semantic checker; structural replay is not acceptance"))?);
            if binding["admitted_checker"]["sha256"]
                != json!(format!("sha256:{:x}", Sha256::digest(fs::read(&tool)?)))
            {
                return Err(invalid(
                    "Admitted semantic checker identity changed; replay refused",
                ));
            }
            let admission = owner.join("owner-admission.json");
            for attempt in report["attempts"]
                .as_array()
                .ok_or_else(|| invalid("No completed attempts"))?
            {
                let input = attempt["check_input"]
                    .as_str()
                    .ok_or_else(|| invalid("Replay input missing"))?;
                if !artifacts
                    .iter()
                    .any(|item| item["path"].as_str() == Some(input))
                {
                    return Err(invalid("Unrecorded replay input"));
                }
                let mut command = Command::new(&tool);
                command
                    .arg("check-admitted")
                    .arg(input)
                    .arg(&admission)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                #[cfg(unix)]
                command.process_group(0);
                let child = command.spawn()?;
                let guard = ToolGuard {
                    pid: child.id(),
                    children: Arc::clone(&self.children),
                };
                self.children
                    .lock()
                    .map_err(|_| invalid("Process lock poisoned"))?
                    .insert(child.id());
                let (stdout, _, code) =
                    run_child(child, DEFAULT_TIMEOUT_MS, &AtomicBool::new(false))?;
                drop(guard);
                let decision: Value = serde_json::from_str(&stdout)?;
                if code != 0 && code != 1 {
                    return Err(invalid("Replay checker failed"));
                }
                if decision != attempt["decision"] {
                    return Err(invalid("Replay decision differs from retained decision"));
                }
                decisions.push(json!({"id":attempt["id"],"decision":decision}));
            }
        }
        Ok(
            json!({"mode":"evidence-recheck","effectful_rerun":false,"job":job,"decisions":decisions,"hashes_verified":artifacts.len()}),
        )
    }

    fn draft_save(&mut self, args: &Value) -> Result<Value> {
        let id = format!("phod-draft-{}", stamp());
        safe_component(&id, "draft id")?;
        let (source, source_hash, source_size) =
            if let Some(path) = args.get("source_path").and_then(Value::as_str) {
                let source = PathBuf::from(path);
                let destination = self.root.join("archives").join(format!("{id}.phod"));
                let (size, hash) = copy_archive(&source, &destination)?;
                (destination, hash, size)
            } else {
                let text = draft_text(args)?;
                let destination = self.root.join("archives").join(format!("{id}.phod"));
                atomic_write(&destination, text.as_bytes())?;
                (
                    destination,
                    format!("sha256:{:x}", Sha256::digest(text.as_bytes())),
                    text.len() as u64,
                )
            };
        let record = json!({"id":id,"kind":"draft","state":"saved","created_at":now(),"source_path":source,"source_hash":source_hash,"source_size":source_size});
        atomic_write(
            &self.root.join("drafts").join(format!("{id}.json")),
            serde_json::to_vec(&record)?.as_slice(),
        )?;
        Ok(record)
    }

    fn start(&mut self, command: &str, args: &Value) -> Result<Value> {
        let current = self
            .jobs
            .lock()
            .map_err(|_| invalid("Phod job lock poisoned"))?;
        if current
            .values()
            .filter(|job| matches!(job.value["state"].as_str(), Some("queued" | "running")))
            .count()
            >= MAX_ACTIVE_JOBS
        {
            return Err(invalid(
                "Four Phod jobs are active. Wait for a job to finish before starting another.",
            ));
        }
        drop(current);
        let action = command.strip_prefix("phod-").unwrap_or(command).to_owned();
        let id = format!("phod-job-{}", stamp());
        safe_component(&id, "job id")?;
        let source = if let Some(path) = args.get("source_path").and_then(Value::as_str) {
            PathBuf::from(path)
        } else if let Some(draft) = args.get("draft_id").and_then(Value::as_str) {
            safe_component(draft, "draft id")?;
            self.root.join("archives").join(format!("{draft}.phod"))
        } else {
            return Err(invalid("Phod action requires source_path or draft_id"));
        };
        let archive = self.root.join("archives").join(format!("{id}.phod"));
        let (size, hash) = copy_archive(&source, &archive)?;
        let record = json!({"id":id,"kind":"job","action":action,"state":"queued","created_at":now(),"source_path":archive,"source_hash":hash,"source_size":size,"evidence":{}});
        atomic_write(
            &self.root.join("jobs").join(format!("{id}.json")),
            serde_json::to_vec(&record)?.as_slice(),
        )?;
        self.jobs
            .lock()
            .map_err(|_| invalid("Phod job lock poisoned"))?
            .insert(
                id.clone(),
                Job {
                    value: record.clone(),
                },
            );
        let jobs = Arc::clone(&self.jobs);
        let root = self.root.clone();
        let args = args.clone();
        let worker_id = id.clone();
        let worker_action = action.clone();
        let worker_archive = archive.clone();
        let worker_record = record.clone();
        let children = Arc::new(Mutex::new(BTreeSet::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        self.controls
            .insert(id.clone(), (Arc::clone(&cancelled), Arc::clone(&children)));
        thread::spawn(move || {
            let result = run_job(
                &root,
                &worker_id,
                &worker_action,
                &worker_archive,
                &args,
                &children,
                &cancelled,
            );
            let mut value = worker_record;
            match result {
                Ok(evidence) => {
                    let failed = [
                        "exit_code",
                        "derive_exit_code",
                        "verify_exit_code",
                        "charter_exit_code",
                    ]
                    .iter()
                    .any(|key| evidence[*key].as_i64().is_some_and(|code| code != 0));
                    value["state"] = json!(if failed { "failed" } else { "succeeded" });
                    if worker_action == "workbench"
                        && let Some(stdout) = evidence["stdout"].as_str()
                        && let Ok(receipt) = serde_json::from_str::<Value>(stdout)
                        && matches!(receipt["status"].as_str(), Some("blocked" | "unavailable"))
                    {
                        value["state"] = receipt["status"].clone();
                    }
                    if failed {
                        value["error"] = json!(
                            "The tool reported a failing check. Inspect the recorded diagnostics."
                        );
                    }
                    value["evidence"] = evidence;
                }
                Err(error) => {
                    value["state"] = json!(if error.to_string().starts_with("unavailable:") {
                        "unavailable"
                    } else {
                        "failed"
                    });
                    value["error"] = json!(error.to_string());
                }
            }
            if cancelled.load(Ordering::SeqCst) {
                value["state"] = json!("cancelled");
                value["error"] = json!("Cancelled by owner; source and partial outputs retained");
            }
            if value["evidence"]["artifacts"].is_null() {
                let mut items = vec![];
                if let Ok(item) = artifact(&worker_archive, "input") {
                    items.push(item);
                }
                let output = root.join("outputs").join(&worker_id);
                if output.is_dir() {
                    let _ = collect_artifacts(&output, &mut items);
                }
                let owner = root.join("admissions").join(&worker_id);
                if owner.is_dir() {
                    let _ = collect_artifacts(&owner, &mut items);
                }
                value["evidence"]["artifacts"] = json!(items);
            }
            value["completed_at"] = json!(now());
            if let Err(error) = atomic_write(
                &root.join("jobs").join(format!("{worker_id}.json")),
                serde_json::to_vec(&value).unwrap_or_default().as_slice(),
            ) {
                value["state"] = json!("failed");
                value["error"] = json!(format!(
                    "Could not durably save the tool result: {error}. Source archive was preserved."
                ));
            }
            if let Ok(mut map) = jobs.lock() {
                map.insert(worker_id, Job { value });
            }
        });
        Ok(json!({"id":id,"state":"queued","action":action}))
    }
}

fn tooling() -> Value {
    json!({"parser": find_tool("PHOD_PARSER_BIN", "phod-parser"), "derive": find_tool("PHOD_DERIVE_BIN", "phod-derive"), "verify": find_tool("PHOD_VERIFY_BIN", "phod-verify"), "charter": find_tool("PHOD_CHARTER_BIN", "phod-charter"), "zig": find_tool("ZIG_BIN", "zig"), "workbench":find_tool("PHOD_WORKBENCH_BIN","phod-workbench"), "semantic":find_tool("PHOD_SEMANTIC_BIN","phod-semantic"), "owner":find_tool("PHOD_OWNER_BIN","phod-owner"), "rustc":json!({"available":rustc_path().is_some(),"path":rustc_path()})})
}

fn capabilities(tools: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for (action, required) in [
        ("validate", vec!["parser"]),
        ("compile", vec!["parser", "derive"]),
        ("verify", vec!["parser", "verify"]),
        ("charter", vec!["parser", "charter"]),
        ("workbench", vec!["workbench", "semantic", "owner", "rustc"]),
    ] {
        let missing: Vec<_> = required
            .iter()
            .filter(|tool| tools[**tool]["available"] != true)
            .copied()
            .collect();
        result.insert(
            action.into(),
            json!({"available":missing.is_empty(),"required":required,"missing":missing}),
        );
    }
    Value::Object(result)
}

fn find_tool(variable: &str, command: &str) -> Value {
    if let Some(path) = env::var_os(variable) {
        let path = PathBuf::from(path);
        return json!({"path":path,"available":path.is_absolute()&&path.is_file(),"source":format!("configured {variable}")});
    }
    let mut roots = Vec::new();
    if let Some(root) = env::var_os("QROC_RESOURCE_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if let Ok(exe) = env::current_exe()
        && let Some(root) = exe.parent().and_then(Path::parent)
    {
        roots.push(root.to_owned());
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for root in roots {
        let runtime = root.join("runtime/phod");
        let rel = match command {
            "phod-owner" => Some("owner.py"),
            "phod-workbench" => Some("workbench.py"),
            "phod-semantic" => Some("bin/phod-semantic"),
            _ => None,
        };
        if let Some(rel) = rel {
            let path = runtime.join(rel);
            if path.is_file() {
                let actual = read_bounded(&path)
                    .ok()
                    .map(|b| format!("{:x}", Sha256::digest(&b)));
                let expected = fs::read_to_string(runtime.join("manifest.sha256"))
                    .ok()
                    .and_then(|s| {
                        s.lines().find_map(|l| {
                            let mut p = l.split_whitespace();
                            let h = p.next()?;
                            let n = p.next()?;
                            (n == rel).then(|| h.to_owned())
                        })
                    });
                return json!({"path":path,"available":expected.is_some()&&expected==actual,"sha256":actual,"source":"standalone selected PHOD leaf"});
            }
        }
        let path = root.join("ui/modules/bin").join(command);
        if !path.is_file() {
            continue;
        }
        let manifest = root.join("ui/modules/phod-toolchain.sha256");
        let expected = fs::read_to_string(&manifest).ok().and_then(|text| {
            text.lines().find_map(|line| {
                let mut parts = line.split_whitespace();
                let hash = parts.next()?;
                let name = parts.next()?;
                (name == command).then(|| hash.to_owned())
            })
        });
        let actual = File::open(&path).ok().and_then(|mut file| {
            let mut hasher = Sha256::new();
            let mut chunk = [0u8; 65536];
            loop {
                let n = file.read(&mut chunk).ok()?;
                if n == 0 {
                    break;
                }
                hasher.update(&chunk[..n]);
            }
            Some(format!("{:x}", hasher.finalize()))
        });
        let verified = expected.is_some() && expected == actual;
        return json!({"path":path,"available":verified,"sha256":actual,"source":"bundled upstream Phod","error":if verified{Value::Null}else{json!("Tool digest does not match the release manifest")}});
    }
    json!({"path":Value::Null,"available":false,"source":"No bundled or explicitly configured tool"})
}

fn configured_tool(variable: &str, command: &str) -> Result<PathBuf> {
    let item = find_tool(variable, command);
    if item.get("available").and_then(Value::as_bool) != Some(true) {
        return Err(invalid(format!(
            "unavailable: Phod tool {command} is not installed; set {variable} or install the original toolchain"
        )));
    }
    item.get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| {
            invalid(format!(
                "unavailable: Phod tool {command} path is not configured"
            ))
        })
}

fn read_pipe(pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.take((MAX_TOOL_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .ok();

        bytes
    })
}

fn run_child(
    mut child: Child,
    timeout_ms: u64,
    cancelled: &AtomicBool,
) -> Result<(String, String, i32)> {
    let out = read_pipe(
        child
            .stdout
            .take()
            .ok_or_else(|| invalid("Phod tool stdout unavailable"))?,
    );
    let err = read_pipe(
        child
            .stderr
            .take()
            .ok_or_else(|| invalid("Phod tool stderr unavailable"))?,
    );
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            stop_group(child.id());
            let stdout = out.join().unwrap_or_default();
            let stderr = err.join().unwrap_or_default();
            if stdout.len() > MAX_TOOL_OUTPUT || stderr.len() > MAX_TOOL_OUTPUT {
                return Err(invalid(
                    "Phod tool output exceeded its 4 MiB bound; no successful result was accepted.",
                ));
            }
            let stdout = String::from_utf8_lossy(&stdout).into_owned();
            let stderr = String::from_utf8_lossy(&stderr).into_owned();
            return Ok((stdout, stderr, status.code().unwrap_or(1)));
        }
        if cancelled.load(Ordering::SeqCst)
            || start.elapsed() > Duration::from_millis(timeout_ms.clamp(1, DEFAULT_TIMEOUT_MS))
        {
            stop_group(child.id());
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid("Phod tool exceeded its bounded timeout"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn artifact(path: &Path, role: &str) -> Result<Value> {
    regular_source(path)?;
    let bytes = read_bounded(path)?;
    Ok(
        json!({"path":path,"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"role":role}),
    )
}
fn collect_artifacts(root: &Path, items: &mut Vec<Value>) -> Result<()> {
    for entry in fs::read_dir(root)? {
        if items.len() >= 128 {
            return Err(invalid("Phod produced too many artifacts"));
        }
        let path = entry?.path();
        reject_symlink_components(&path)?;
        if path.is_dir() {
            collect_artifacts(&path, items)?;
        } else {
            items.push(artifact(&path, "output")?);
        }
    }
    Ok(())
}
fn run_job(
    root: &Path,
    id: &str,
    action: &str,
    archive: &Path,
    args: &Value,
    children: &Arc<Mutex<BTreeSet<u32>>>,
    cancelled: &AtomicBool,
) -> Result<Value> {
    let timeout = args["timeout_ms"]
        .as_u64()
        .unwrap_or(DEFAULT_TIMEOUT_MS)
        .clamp(1, DEFAULT_TIMEOUT_MS);
    let output = root.join("outputs").join(id);
    reject_symlink_components(&output)?;
    fs::create_dir(&output)?;
    if cancelled.load(Ordering::SeqCst) {
        return Err(invalid("Cancelled before execution"));
    }
    if action == "workbench" {
        if rustc_path().is_none() {
            return Err(invalid("unavailable: local Rust compiler"));
        }
        let owner = root.join("admissions").join(id);
        fs::create_dir(&owner)?;
        let owner_tool = configured_tool("PHOD_OWNER_BIN", "phod-owner")?;
        let (child, guard) = spawn_tool(&owner_tool, archive, &owner, children)?;
        let (owner_stdout, owner_stderr, owner_code) = run_child(child, timeout, cancelled)?;
        drop(guard);
        if owner_code != 0 {
            let mut items = vec![artifact(archive, "input")?];
            collect_artifacts(&owner, &mut items)?;
            return Ok(
                json!({"action":action,"stdout":owner_stdout,"stderr":owner_stderr,"exit_code":owner_code,"artifacts":items,"output_dir":output,"owner_directory":owner}),
            );
        }
        if cancelled.load(Ordering::SeqCst) {
            return Err(invalid("Cancelled before candidate collection"));
        }
        let tool = configured_tool("PHOD_WORKBENCH_BIN", "phod-workbench")?;
        let (child, guard) = spawn_tool(&tool, archive, &output, children)?;
        let (stdout, stderr, code) = run_child(child, timeout, cancelled)?;
        drop(guard);
        let mut items = vec![artifact(archive, "input")?];
        collect_artifacts(&output, &mut items)?;
        collect_artifacts(&owner, &mut items)?;
        return Ok(
            json!({"action":action,"owner_directory":owner,"tool":tool,"stdout":stdout,"stderr":stderr,"exit_code":code,"output_dir":output,"artifacts":items}),
        );
    }
    let parser = configured_tool("PHOD_PARSER_BIN", "phod-parser")?;
    let (child, guard) = spawn_tool(&parser, archive, &output, children)?;
    let (canonical, diagnostics, code) = run_child(child, timeout, cancelled)?;
    drop(guard);
    let canonical_path = output.join("canonical.json");
    atomic_write(&canonical_path, canonical.as_bytes())?;
    let mut evidence = json!({"parser":parser,"canonical_path":canonical_path,"output_dir":output,"stdout":canonical,"stderr":diagnostics,"exit_code":code,"action":action});
    if code == 0 && action != "validate" {
        if cancelled.load(Ordering::SeqCst) {
            return Err(invalid("Cancelled before next tool"));
        }
        let parsed: Value = serde_json::from_str(&canonical)?;
        safe_component(
            parsed["intent"]["name"]
                .as_str()
                .ok_or_else(|| invalid("Canonical Phod intent name is missing"))?,
            "canonical intent name",
        )?;
        let (variable, name, prefix) = match action {
            "compile" => ("PHOD_DERIVE_BIN", "phod-derive", "derive"),
            "charter" => ("PHOD_CHARTER_BIN", "phod-charter", "charter"),
            "verify" => ("PHOD_VERIFY_BIN", "phod-verify", "verify"),
            _ => return Err(invalid("Unsupported Phod action")),
        };
        let tool = configured_tool(variable, name)?;
        let (child, guard) = spawn_tool(&tool, &canonical_path, &output, children)?;
        let (stdout, stderr, code) = run_child(child, timeout, cancelled)?;
        drop(guard);
        evidence[prefix] = json!(tool);
        evidence[format!("{prefix}_stdout")] = json!(stdout);
        evidence[format!("{prefix}_stderr")] = json!(stderr);
        evidence[format!("{prefix}_exit_code")] = json!(code);
    }
    let mut artifacts = vec![artifact(archive, "input")?];
    collect_artifacts(&output, &mut artifacts)?;
    evidence["artifacts"] = json!(artifacts);
    Ok(evidence)
}
