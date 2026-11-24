//! The media project type: a track or a piece with its brief, lyrics, concepts and versions.
//!
//! Documents are revisioned records beside the Q-ROC store. A version is a reference to a file
//! where it already lies: the file is never moved, copied or written; its size and sha256 are
//! recorded when it is referenced and compared each time the versions are listed.
use crate::qroc::QrocStore;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The documents of a media project, in reading order.
pub const DOCUMENTS: [&str; 3] = ["brief", "lyrics", "concepts"];
const MAX_TEXT: usize = 1024 * 1024;
const MAX_VERSIONS: usize = 512;
/// Environment setting naming the media store; unset, the project folder's media/ is the store.
pub const MEDIA_ROOT_ENV: &str = "QROC_MEDIA_ROOT";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaDocument {
    pub project_id: String,
    pub document: String,
    pub revision: u32,
    pub content: String,
    pub sha256: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaVersion {
    pub number: u32,
    pub name: String,
    pub reference: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub note: String,
    pub referenced_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Versions {
    versions: Vec<MediaVersion>,
}

fn invalid(s: impl Into<String>) -> Error {
    Error::Invalid(s.into())
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn component(value: &str, name: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(invalid(format!("Invalid {name}")));
    }
    Ok(value.to_owned())
}
fn document_name(value: &str) -> Result<&'static str> {
    DOCUMENTS
        .into_iter()
        .find(|d| *d == value.to_ascii_lowercase())
        .ok_or_else(|| invalid("Media document must be brief, lyrics or concepts"))
}
fn title(document: &str) -> String {
    let mut c = document.chars();
    c.next()
        .map(|f| format!("{}{}", f.to_ascii_uppercase(), c.as_str()))
        .unwrap_or_default()
}
fn project_dir(root: &Path, project_id: &str) -> Result<PathBuf> {
    let dir = root
        .join("media")
        .join(component(project_id, "project_id")?);
    crate::scope::validate_root(&dir)?;
    Ok(dir)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::scope::validate_root(path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn write_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    crate::scope::validate_root(path)?;
    let temp = path.with_extension("tmp");
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    write_new(&temp, bytes)?;
    fs::rename(temp, path)?;
    Ok(())
}

/// The media store for a project: the setting when given, else the project folder's media/.
pub fn media_root(root: &Path, project_title: &str, setting: Option<&Path>) -> Result<PathBuf> {
    match setting {
        Some(p) if !p.is_absolute() => Err(invalid("The media root setting must be absolute")),
        Some(p) => Ok(p.to_owned()),
        None => Ok(root
            .join("project-folders")
            .join(project_title)
            .join("media")),
    }
}

fn documents(root: &Path, project_id: &str, document: &str) -> Result<Vec<MediaDocument>> {
    let dir = project_dir(root, project_id)?.join("documents");
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with(&format!("{document}-r")) || !name.ends_with(".json") {
            continue;
        }
        let doc: MediaDocument = serde_json::from_slice(&fs::read(&path)?)?;
        if doc.project_id != project_id
            || doc.document != document
            || digest(doc.content.as_bytes()) != doc.sha256
        {
            return Err(invalid("Media document owner or digest mismatch"));
        }
        out.push(doc);
    }
    out.sort_by_key(|d| d.revision);
    Ok(out)
}

fn save_document(
    root: &Path,
    project_id: &str,
    document: &str,
    content: &str,
) -> Result<MediaDocument> {
    let document = document_name(document)?;
    if content.len() > MAX_TEXT {
        return Err(invalid("Media document exceeds 1 MiB"));
    }
    let revision = documents(root, project_id, document)?
        .last()
        .map_or(0, |d| d.revision)
        + 1;
    let doc = MediaDocument {
        project_id: project_id.to_owned(),
        document: document.to_owned(),
        revision,
        content: content.to_owned(),
        sha256: digest(content.as_bytes()),
        created_at: now(),
    };
    let path = project_dir(root, project_id)?
        .join("documents")
        .join(format!("{document}-r{revision}.json"));
    write_new(&path, &serde_json::to_vec_pretty(&doc)?)?;
    Ok(doc)
}

fn load_versions(root: &Path, project_id: &str) -> Result<Versions> {
    let path = project_dir(root, project_id)?.join("versions.json");
    if !path.exists() {
        return Ok(Versions::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

/// A reference must be an absolute regular file inside the media store, with no symlink on the way.
fn check_reference(path: &Path, store: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid("A version file must be an absolute path"));
    }
    let mut current = PathBuf::new();
    for c in path.components() {
        current.push(c);
        if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid("A version file path may not contain symlinks"));
        }
    }
    if !path.starts_with(store) {
        return Err(invalid("A version file must lie in the media store"));
    }
    if !fs::metadata(path)?.is_file() {
        return Err(invalid("A version must reference a regular file"));
    }
    Ok(())
}

/// Duration of a PCM WAV from its header alone; other formats report none.
fn wav_duration(path: &Path) -> Option<f64> {
    let mut head = [0u8; 4096];
    let n = fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..n];
    if head.len() < 12 || &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let (mut at, mut byte_rate, mut data) = (12usize, 0u32, None);
    while at + 8 <= head.len() {
        let id = &head[at..at + 4];
        let size = u32::from_le_bytes(head[at + 4..at + 8].try_into().ok()?);
        if id == b"fmt " && at + 20 <= head.len() {
            byte_rate = u32::from_le_bytes(head[at + 16..at + 20].try_into().ok()?);
        }
        if id == b"data" {
            data = Some(size);
            break;
        }
        at = at.checked_add(8 + size as usize + (size as usize & 1))?;
    }
    let data = data?;
    (byte_rate > 0).then(|| f64::from(data) / f64::from(byte_rate))
}

/// The state of each version against its recorded size and sha256: unchanged, changed or missing.
fn version_state(v: &MediaVersion) -> Value {
    let Ok(meta) = fs::metadata(&v.reference) else {
        return json!({"state":"missing","bytes":null,"sha256":null});
    };
    if !meta.is_file() {
        return json!({"state":"missing","bytes":null,"sha256":null});
    }
    match hash_file(&v.reference) {
        Ok(sha) => json!({
            "state": if sha == v.sha256 && meta.len() == v.bytes {"unchanged"} else {"changed"},
            "bytes": meta.len(),
            "sha256": sha,
        }),
        Err(_) => json!({"state":"missing","bytes":meta.len(),"sha256":null}),
    }
}

fn media_project<'a>(qroc: &'a QrocStore, args: &Value) -> Result<&'a crate::qroc::Project> {
    let project = qroc.project(s(args, "project_id"))?;
    if project.kind != "media" {
        return Err(invalid("This project is not a media project"));
    }
    Ok(project)
}

/// One version, checked against its recorded bytes, for a caller that acts on the file.
pub fn version(qroc: &QrocStore, project_id: &str, number: u64) -> Result<(MediaVersion, Value)> {
    media_project(qroc, &json!({"project_id":project_id}))?;
    let v = load_versions(qroc.root(), project_id)?
        .versions
        .into_iter()
        .find(|v| u64::from(v.number) == number)
        .ok_or_else(|| invalid("Version not found"))?;
    let state = version_state(&v);
    Ok((v, state))
}

/// Library entry for the `qroc-media-*` commands. `setting` is the media root setting, if any.
pub fn command(
    qroc: &mut QrocStore,
    setting: Option<&Path>,
    command: &str,
    args: &Value,
) -> Result<Value> {
    match command {
        "qroc-media-create" => {
            let brief = s(args, "brief");
            if brief.trim().is_empty() {
                return Err(invalid("A media project starts from its brief"));
            }
            let project = qroc.command(
                "qroc-project-create",
                &json!({"title":s(args,"title"),"brain_dump":brief,"kind":"media"}),
            )?;
            let id = project["id"].as_str().unwrap_or_default().to_owned();
            let root = qroc.root().to_owned();
            let mut saved = vec![save_document(&root, &id, "brief", brief)?];
            for d in ["lyrics", "concepts"] {
                if !s(args, d).is_empty() {
                    saved.push(save_document(&root, &id, d, s(args, d))?);
                }
            }
            Ok(json!({"project":project,"documents":saved}))
        }
        "qroc-media-document-save" => {
            let project = media_project(qroc, args)?.id.clone();
            Ok(serde_json::to_value(save_document(
                qroc.root(),
                &project,
                s(args, "document"),
                s(args, "content"),
            )?)?)
        }
        "qroc-media-document-get" => {
            let project = media_project(qroc, args)?.id.clone();
            let document = document_name(s(args, "document"))?;
            let all = documents(qroc.root(), &project, document)?;
            let chosen = match args.get("revision").and_then(Value::as_u64) {
                Some(r) => all.iter().find(|d| u64::from(d.revision) == r),
                None => all.last(),
            };
            Ok(
                json!({"document":chosen,"revisions":all.iter().map(|d|d.revision).collect::<Vec<_>>()}),
            )
        }
        "qroc-media-version-add" => {
            let project = media_project(qroc, args)?.clone();
            let name = s(args, "name").trim();
            if name.is_empty()
                || name.len() > 180
                || name
                    .chars()
                    .any(|c| c.is_control() || c == '/' || c == '\\')
            {
                return Err(invalid("A version needs a readable name"));
            }
            let store = media_root(qroc.root(), &project.title, setting)?;
            let path = PathBuf::from(s(args, "path"));
            check_reference(&path, &store)?;
            let mut list = load_versions(qroc.root(), &project.id)?;
            if list.versions.len() >= MAX_VERSIONS {
                return Err(invalid("Version limit reached"));
            }
            if list.versions.iter().any(|v| v.name == name) {
                return Err(invalid("A version with this name already exists"));
            }
            let bytes = fs::metadata(&path)?.len();
            let version = MediaVersion {
                number: list.versions.last().map_or(0, |v| v.number) + 1,
                name: name.to_owned(),
                sha256: hash_file(&path)?,
                duration_seconds: wav_duration(&path),
                reference: path,
                bytes,
                note: s(args, "note").chars().take(2000).collect(),
                referenced_at: now(),
            };
            list.versions.push(version.clone());
            write_replace(
                &project_dir(qroc.root(), &project.id)?.join("versions.json"),
                &serde_json::to_vec_pretty(&list)?,
            )?;
            Ok(serde_json::to_value(version)?)
        }
        "qroc-media-version-list" => {
            let project = media_project(qroc, args)?.id.clone();
            let versions = load_versions(qroc.root(), &project)?
                .versions
                .iter()
                .map(|v| {
                    let mut row = serde_json::to_value(v)?;
                    row["now"] = version_state(v);
                    row["state"] = row["now"]["state"].clone();
                    Ok(row)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"project_id":project,"versions":versions}))
        }
        _ => Err(invalid(format!("Unknown media command: {command}"))),
    }
}

/// Readable media/ files for the project folder: names, never paths.
pub(crate) fn readable(root: &Path, project_id: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for document in DOCUMENTS {
        let all = documents(root, project_id, document)?;
        let name = title(document);
        let last = all.len().saturating_sub(1);
        for (i, d) in all.iter().enumerate() {
            let path = if i == last {
                format!("media/{name}.md")
            } else {
                format!("media/revisions/{name} - revision {}.md", d.revision)
            };
            out.push((path, d.content.clone()));
        }
    }
    let versions = load_versions(root, project_id)?.versions;
    let mut text = String::from("# Versions\n\n");
    if versions.is_empty() {
        text.push_str("No version yet.\n");
    }
    for v in &versions {
        let state = version_state(v);
        let file = v
            .reference
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        text.push_str(&format!(
            "## {}. {}\n\n- File: {}\n- State: {}\n- Size when referenced: {} bytes\n- sha256 when referenced: {}\n",
            v.number, v.name, file, state["state"].as_str().unwrap_or("missing"), v.bytes, v.sha256
        ));
        if let Some(d) = v.duration_seconds {
            text.push_str(&format!("- Duration: {d:.2} s\n"));
        }
        if !v.note.is_empty() {
            text.push_str(&format!("- Note: {}\n", v.note));
        }
        text.push('\n');
    }
    out.push(("media/Versions.md".into(), text));
    Ok(out)
}
