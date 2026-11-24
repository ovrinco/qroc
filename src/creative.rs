//! Versioned, local-only creative preparation. No provider or publication effects.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};
const TOOL: &str = "/usr/bin/ffmpeg";
const MAX_INPUT: u64 = 64 * 1024 * 1024;
#[derive(Default, Serialize, Deserialize)]
struct State {
    schema: u32,
    serial: u64,
    assets: Vec<Value>,
    outputs: Vec<Value>,
    exports: Vec<Value>,
}
struct Active {
    id: String,
    child: Arc<Mutex<Child>>,
    done: Arc<(Mutex<bool>, Condvar)>,
    watchdog: Option<JoinHandle<()>>,
}
impl Active {
    fn new(id: String, child: Child) -> Self {
        let child = Arc::new(Mutex::new(child));
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let c = child.clone();
        let d = done.clone();
        let watchdog = thread::spawn(move || {
            let (lock, wake) = &*d;
            let stopped = lock.lock().unwrap();
            let (stopped, timeout) = wake
                .wait_timeout_while(stopped, Duration::from_secs(20), |done| !*done)
                .unwrap();
            if timeout.timed_out() && !*stopped {
                let _ = c.lock().unwrap().kill();
            }
        });
        Self {
            id,
            child,
            done,
            watchdog: Some(watchdog),
        }
    }
    fn stop_watchdog(&mut self) {
        let (lock, wake) = &*self.done;
        *lock.lock().unwrap() = true;
        wake.notify_all();
        if let Some(t) = self.watchdog.take() {
            let _ = t.join();
        }
    }
    fn kill_wait(&mut self) -> Result<()> {
        self.stop_watchdog();
        let mut child = self.child.lock().unwrap();
        if child.try_wait()?.is_none() {
            child.kill()?;
        }
        child.wait()?;
        Ok(())
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        let _ = self.kill_wait();
    }
}
pub struct CreativeStore {
    root: PathBuf,
    state: State,
    active: Option<Active>,
}
fn invalid(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or_else(|| invalid(format!("Missing or invalid {key}")))
}
fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn safe(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid("Path must be absolute without parent traversal"));
    }
    let mut current = PathBuf::new();
    for c in path.components() {
        current.push(c);
        if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid("Creative paths may not contain symlinks"));
        }
    }
    Ok(())
}
impl CreativeStore {
    pub fn open(root: &Path) -> Result<Self> {
        safe(root)?;
        // Discovery alone does not create a creative project or run a tool.
        let path = root.join("state.json");
        safe(&path)?;
        let state = if path.exists() {
            if fs::metadata(&path)?.len() > 4 * 1024 * 1024 {
                return Err(invalid("Creative state exceeds 4 MiB"));
            }
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            State {
                schema: 1,
                ..State::default()
            }
        };
        let mut store = Self {
            root: root.to_owned(),
            state,
            active: None,
        };
        if store.state.schema != 1 {
            return Err(invalid("Unsupported creative schema"));
        }
        let mut changed = false;
        for i in 0..store.state.outputs.len() {
            if store.state.outputs[i]["status"] == "running" {
                store.state.outputs[i]["status"] = json!("interrupted");
                store.state.outputs[i]["error"] = json!(
                    "Owner exited before completion; partial bytes are not an accepted output"
                );
                store.partial(i)?;
                changed = true;
            }
        }
        if changed {
            store.save()?
        }
        Ok(store)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Existing local adapter discovery, never conversion or provider launch.
    pub fn available(&self) -> bool {
        self.tool().is_ok()
    }
    fn path(&self, rel: &str) -> Result<PathBuf> {
        if rel.contains('/') || rel.contains('\\') || rel.starts_with('.') || rel.is_empty() {
            return Err(invalid("Invalid creative file identity"));
        }
        let p = self.root.join(rel);
        safe(&p)?;
        Ok(p)
    }
    fn save(&self) -> Result<()> {
        safe(&self.root)?;
        fs::create_dir_all(&self.root)?;
        let bytes = serde_json::to_vec_pretty(&self.state)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(invalid("Creative state exceeds 4 MiB"));
        }
        let temp = self.path("state.tmp")?;
        let mut f = fs::File::create(&temp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(temp, self.path("state.json")?)?;
        fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }
    fn next(&mut self, prefix: &str) -> String {
        self.state.serial += 1;
        format!("{prefix}-{}", self.state.serial)
    }
    fn index(&self, args: &Value) -> Result<usize> {
        let id = text(args, "artifact_id")?;
        let revision = args["revision"]
            .as_u64()
            .ok_or_else(|| invalid("Exact output revision required"))?;
        self.state
            .outputs
            .iter()
            .position(|o| o["artifact_id"] == id && o["revision"] == revision)
            .ok_or_else(|| invalid("Output revision not found"))
    }
    fn partial(&mut self, i: usize) -> Result<()> {
        let p = self.path(text(&self.state.outputs[i], "file")?)?;
        self.state.outputs[i]["partial"] = if p.exists() {
            json!({"file":p,"bytes":fs::metadata(&p)?.len(),"sha256":hash(&p)?,"accepted":false})
        } else {
            Value::Null
        };
        Ok(())
    }
    fn finish(&mut self, i: usize, status: &str, error: Value) -> Result<()> {
        self.state.outputs[i]["status"] = json!(status);
        self.state.outputs[i]["error"] = error;
        let log = self.path(text(&self.state.outputs[i], "log_file")?)?;
        if log.exists() {
            let mut bytes = Vec::new();
            fs::File::open(log)?.take(16384).read_to_end(&mut bytes)?;
            self.state.outputs[i]["tool_error"] = json!(String::from_utf8_lossy(&bytes));
        }
        if status == "succeeded" {
            let p = self.path(text(&self.state.outputs[i], "file")?)?;
            self.state.outputs[i]["output_sha256"] = json!(hash(&p)?);
            self.state.outputs[i]["output_bytes"] = json!(fs::metadata(p)?.len());
        } else {
            self.partial(i)?
        }
        self.save()
    }
    fn poll(&mut self) -> Result<()> {
        if let Some(mut active) = self.active.take() {
            let i = self
                .state
                .outputs
                .iter()
                .position(|o| o["job_id"] == active.id)
                .ok_or_else(|| invalid("Missing running job"))?;
            let status = active.child.lock().unwrap().try_wait()?;
            if let Some(status) = status {
                active.stop_watchdog();
                let success = status.success()
                    && fs::metadata(self.path(text(&self.state.outputs[i], "file")?)?)
                        .is_ok_and(|m| m.len() > 44);
                self.finish(
                    i,
                    if success { "succeeded" } else { "failed" },
                    if success {
                        Value::Null
                    } else {
                        json!(format!(
                            "ffmpeg exited {status} (20-second watchdog active); inspect tool_error"
                        ))
                    },
                )?;
            } else {
                self.active = Some(active);
            }
        }
        Ok(())
    }
    fn tool(&self) -> Result<Value> {
        let out = Command::new(TOOL)
            .arg("-version")
            .stdin(Stdio::null())
            .output()?;
        if !out.status.success() {
            return Err(invalid("ffmpeg version discovery failed"));
        }
        Ok(
            json!({"id":"ffmpeg-audio-wav-v1","path":TOOL,"version":String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or(""),"sha256":hash(Path::new(TOOL))?}),
        )
    }
    /// Trims a media project's version from the file it references, without copying that file.
    /// The caller has checked the file against the size and sha256 recorded with the version.
    pub fn trim_reference(
        &mut self,
        project_id: &str,
        version: u32,
        reference: &Path,
        sha256: &str,
        args: &Value,
    ) -> Result<Value> {
        self.poll()?;
        safe(reference)?;
        let meta = fs::metadata(reference)?;
        if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_INPUT {
            return Err(invalid(
                "A version file to trim must be a nonempty regular file up to 64 MiB",
            ));
        }
        if self.state.assets.len() >= 256 {
            return Err(invalid("Source version limit reached"));
        }
        let asset = format!("media-version-{version}");
        let revision = self
            .state
            .assets
            .iter()
            .filter(|a| a["asset_id"] == asset.as_str() && a["project_id"] == project_id)
            .filter_map(|a| a["revision"].as_u64())
            .max()
            .unwrap_or(0)
            + 1;
        self.state.assets.push(json!({"asset_id":asset,"revision":revision,"project_id":project_id,"branch_id":"media","reference":reference,"file":null,"bytes":meta.len(),"sha256":sha256}));
        self.save()?;
        let mut start = args.clone();
        start["project_id"] = json!(project_id);
        start["branch_id"] = json!("media");
        start["asset_id"] = json!(asset);
        start["source_revision"] = json!(revision);
        start["adapter_id"] = json!("ffmpeg-audio-wav-v1");
        self.command("qroc-creative-start", &start)
    }
    pub fn command(&mut self, command: &str, args: &Value) -> Result<Value> {
        self.poll()?;
        match command {
            "qroc-creative-capabilities" => {
                let tool = self.tool();
                let reason = tool.as_ref().err().map(ToString::to_string);
                Ok(json!({"schema":1,"capabilities":[
                    {"id":"ffmpeg-audio-wav-v1","domain":"audio","supported":tool.is_ok(),"reason":reason,"tool":tool.ok(),"input":"Local WAV, MP3, FLAC, Ogg, MOV/MP4, Matroska/WebM, AAC or AIFF file up to 64 MiB; no playlists","output":"Mono PCM WAV, 48 kHz, clip up to 30 seconds","limits":{"one_job":true,"threads":1,"wall_seconds":20}},
                    {"id":"speech-tts","domain":"audio","supported":false,"reason":"No TTS adapter admitted"},
                    {"id":"music-composition","domain":"audio","supported":false,"reason":"No music-generation adapter admitted"},
                    {"id":"video-edit","domain":"video","supported":false,"reason":"Audio extraction is supported; video editing is not"},
                    {"id":"image-art","domain":"image/art","supported":false,"reason":"No image/art adapter admitted"},
                    {"id":"game-prototype","domain":"game","supported":false,"reason":"No game tool adapter admitted"}
                ],"publication":"separate authorized handoff only"}))
            }
            "qroc-creative-list" => {
                let scope = match (args.get("project_id"), args.get("branch_id")) {
                    (None, None) => None,
                    (Some(_), Some(_)) => {
                        Some((text(args, "project_id")?, text(args, "branch_id")?))
                    }
                    _ => {
                        return Err(invalid(
                            "Creative list requires both project_id and branch_id, or neither",
                        ));
                    }
                };
                let filter = |records: &Vec<Value>| {
                    records
                        .iter()
                        .filter(|v| {
                            scope.is_none_or(|(p, b)| v["project_id"] == p && v["branch_id"] == b)
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                };
                Ok(
                    json!({"schema":1,"assets":filter(&self.state.assets),"outputs":filter(&self.state.outputs),"exports":filter(&self.state.exports)}),
                )
            }
            "qroc-creative-source-import" => {
                if self.state.assets.len() >= 256 {
                    return Err(invalid("Source version limit reached"));
                }
                let project = text(args, "project_id")?.to_owned();
                let branch = text(args, "branch_id")?.to_owned();
                let source = Path::new(text(args, "path")?);
                safe(source)?;
                let meta = fs::metadata(source)?;
                if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_INPUT {
                    return Err(invalid(
                        "Source must be a nonempty regular local file up to 64 MiB",
                    ));
                }
                let asset = if let Some(id) = args["asset_id"].as_str() {
                    let prior = self
                        .state
                        .assets
                        .iter()
                        .find(|a| a["asset_id"] == id)
                        .ok_or_else(|| invalid("Asset not found"))?;
                    if prior["project_id"] != project || prior["branch_id"] != branch {
                        return Err(invalid("Asset scope mismatch"));
                    }
                    id.to_owned()
                } else {
                    self.next("asset")
                };
                let revision = self
                    .state
                    .assets
                    .iter()
                    .filter(|a| a["asset_id"] == asset)
                    .filter_map(|a| a["revision"].as_u64())
                    .max()
                    .unwrap_or(0)
                    + 1;
                self.save()?;
                let file = format!("{asset}-r{revision}.source");
                let dest = self.path(&file)?;
                if dest.exists() {
                    return Err(invalid("Source destination already exists"));
                }
                let mut reader = fs::File::open(source)?.take(MAX_INPUT + 1);
                let mut writer = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&dest)?;
                let size = std::io::copy(&mut reader, &mut writer)?;
                writer.sync_all()?;
                if size > MAX_INPUT {
                    return Err(invalid(
                        "Source grew beyond limit; snapshot was not admitted",
                    ));
                }
                let record = json!({"asset_id":asset,"revision":revision,"project_id":project,"branch_id":branch,"original_path":source,"file":file,"bytes":size,"sha256":hash(&dest)?});
                self.state.assets.push(record.clone());
                self.save()?;
                Ok(record)
            }
            "qroc-creative-start" => {
                if self.active.is_some() {
                    return Err(invalid("One creative job is already running"));
                }
                if self.state.outputs.len() >= 256 {
                    return Err(invalid("Output version limit reached"));
                }
                if text(args, "adapter_id")? != "ffmpeg-audio-wav-v1" {
                    return Err(invalid(
                        "capability-unavailable: requested creative adapter is not implemented",
                    ));
                }
                let project = text(args, "project_id")?.to_owned();
                let branch = text(args, "branch_id")?.to_owned();
                let asset = text(args, "asset_id")?;
                let rev = args["source_revision"]
                    .as_u64()
                    .ok_or_else(|| invalid("Exact source_revision required"))?;
                let source = self
                    .state
                    .assets
                    .iter()
                    .find(|a| a["asset_id"] == asset && a["revision"] == rev)
                    .cloned()
                    .ok_or_else(|| invalid("Source revision not found"))?;
                if source["project_id"] != project || source["branch_id"] != branch {
                    return Err(invalid("Source scope mismatch"));
                }
                // A media version is read where it lies; an imported source from its snapshot.
                let input = match source["reference"].as_str() {
                    Some(reference) => {
                        let p = PathBuf::from(reference);
                        safe(&p)?;
                        p
                    }
                    None => self.path(text(&source, "file")?)?,
                };
                if hash(&input)? != source["sha256"].as_str().unwrap_or("") {
                    return Err(invalid("Source snapshot hash mismatch"));
                }
                let number = |k: &str, default: f64, min: f64, max: f64| -> Result<f64> {
                    let n = match args.get(k) {
                        None => default,
                        Some(v) => v.as_f64().ok_or_else(|| invalid(format!("Invalid {k}")))?,
                    };
                    if !n.is_finite() || n < min || n > max {
                        return Err(invalid(format!("{k} must be in {min}..{max}")));
                    }
                    Ok(n)
                };
                let start = number("start_seconds", 0.0, 0.0, 3600.0)?;
                let duration = number("duration_seconds", 10.0, 0.1, 30.0)?;
                let volume = number("volume", 1.0, 0.0, 2.0)?;
                let tool = self.tool()?;
                let artifact = if let Some(id) = args["artifact_id"].as_str() {
                    let p = self
                        .state
                        .outputs
                        .iter()
                        .find(|o| o["artifact_id"] == id)
                        .ok_or_else(|| invalid("Artifact not found"))?;
                    if p["project_id"] != project || p["branch_id"] != branch {
                        return Err(invalid("Artifact scope mismatch"));
                    }
                    id.to_owned()
                } else {
                    self.next("artifact")
                };
                let revision = self
                    .state
                    .outputs
                    .iter()
                    .filter(|o| o["artifact_id"] == artifact)
                    .filter_map(|o| o["revision"].as_u64())
                    .max()
                    .unwrap_or(0)
                    + 1;
                let job = self.next("job");
                let file = format!("{artifact}-r{revision}.wav");
                let log_file = format!("{job}.stderr");
                let output = self.path(&file)?;
                if output.exists() {
                    return Err(invalid("Output destination already exists"));
                }
                let argv = vec![
                    "-nostdin".into(),
                    "-hide_banner".into(),
                    "-loglevel".into(),
                    "error".into(),
                    "-n".into(),
                    "-max_alloc".into(),
                    "67108864".into(),
                    "-threads".into(),
                    "1".into(),
                    "-protocol_whitelist".into(),
                    "file".into(),
                    "-format_whitelist".into(),
                    "wav,mp3,flac,ogg,mov,matroska,webm,aac,aiff".into(),
                    "-ss".into(),
                    start.to_string(),
                    "-i".into(),
                    input.to_string_lossy().into_owned(),
                    "-t".into(),
                    duration.to_string(),
                    "-map".into(),
                    "0:a:0".into(),
                    "-vn".into(),
                    "-af".into(),
                    format!("volume={volume}"),
                    "-filter_threads".into(),
                    "1".into(),
                    "-threads".into(),
                    "1".into(),
                    "-ac".into(),
                    "1".into(),
                    "-ar".into(),
                    "48000".into(),
                    "-c:a".into(),
                    "pcm_s16le".into(),
                    output.to_string_lossy().into_owned(),
                ];
                let record = json!({"job_id":job,"artifact_id":artifact,"revision":revision,"project_id":project,"branch_id":branch,"adapter_id":"ffmpeg-audio-wav-v1","tool":tool,"source":source,"steps":{"start_seconds":start,"duration_seconds":duration,"volume":volume},"argv":argv,"file":file,"log_file":log_file,"status":"running","error":null,"output_sha256":null,"partial":null,"publication":"not-authorized"});
                self.state.outputs.push(record);
                let i = self.state.outputs.len() - 1;
                self.save()?;
                let log = fs::File::create(self.path(&log_file)?)?;
                let mut process = Command::new(TOOL);
                process
                    .args(&argv)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(log);
                // Linux: kill the local tool if the owning bridge crashes, including SIGKILL.
                use std::os::unix::process::CommandExt;
                let parent = std::process::id() as i32;
                unsafe {
                    process.pre_exec(move || {
                        unsafe extern "C" {
                            fn prctl(option: i32, arg: usize, ...) -> i32;
                            fn getppid() -> i32;
                        }
                        if prctl(1, 9usize) != 0 || getppid() != parent {
                            return Err(std::io::Error::other(
                                "Cannot bind ffmpeg lifetime to creative owner",
                            ));
                        }
                        Ok(())
                    });
                }
                match process.spawn() {
                    Ok(child) => {
                        self.active = Some(Active::new(job, child));
                    }
                    Err(e) => {
                        self.finish(i, "failed", json!(e.to_string()))?;
                    }
                }
                Ok(self.state.outputs[i].clone())
            }
            "qroc-creative-status" => {
                let i = self.index(args)?;
                Ok(self.state.outputs[i].clone())
            }
            "qroc-creative-cancel" => {
                let i = self.index(args)?;
                if self.state.outputs[i]["status"] == "running" {
                    if let Some(mut active) = self.active.take() {
                        active.kill_wait()?;
                    }
                    self.finish(
                        i,
                        "cancelled",
                        json!("Cancelled by user; partial bytes retained without acceptance"),
                    )?;
                }
                Ok(self.state.outputs[i].clone())
            }
            "qroc-creative-export" => {
                if self.state.exports.len() >= 256 {
                    return Err(invalid("Export limit reached"));
                }
                let i = self.index(args)?;
                let output = self.state.outputs[i].clone();
                if output["status"] != "succeeded" {
                    return Err(invalid(
                        "Only a succeeded exact output revision may be exported",
                    ));
                }
                let file = self.path(text(&output, "file")?)?;
                if hash(&file)? != output["output_sha256"].as_str().unwrap_or("") {
                    return Err(invalid("Output hash mismatch; export refused"));
                }
                let id = self.next("export");
                self.save()?;
                let dir = self.root.join(&id);
                safe(&dir)?;
                fs::create_dir(&dir)?;
                fs::copy(&file, dir.join("artifact.wav"))?;
                let manifest = json!({"schema":1,"export_id":id,"kind":"creative-proposal","publication":"not-authorized","output":output});
                fs::write(
                    dir.join("manifest.json"),
                    serde_json::to_vec_pretty(&manifest)?,
                )?;
                let record = json!({"export_id":id,"project_id":output["project_id"],"branch_id":output["branch_id"],"artifact_id":output["artifact_id"],"revision":output["revision"],"directory":dir,"output_sha256":hash(&dir.join("artifact.wav"))?,"manifest_sha256":hash(&dir.join("manifest.json"))?});
                self.state.exports.push(record.clone());
                self.save()?;
                Ok(record)
            }
            _ => Err(invalid(
                "capability-unavailable: creative command is not implemented",
            )),
        }
    }
}
impl Drop for CreativeStore {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = active.kill_wait();
            if let Some(i) = self
                .state
                .outputs
                .iter()
                .position(|o| o["job_id"] == active.id)
            {
                let _ = self.finish(
                    i,
                    "interrupted",
                    json!("Owner closed; partial bytes retained without acceptance"),
                );
            }
        }
    }
}
#[cfg(test)]
#[path = "../tests/T07/creative.rs"]
mod tests;
