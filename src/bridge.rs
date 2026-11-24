//! Standalone NDJSON transport; domain payloads remain store-owned.
use crate::{Error, Result, phod::PhodStore, qroc::QrocStore};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{BufRead, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
pub const VERSION: u64 = 1;
pub const MAX_FRAME: usize = 4 * 1024 * 1024;
pub const MAX_REPLY: usize = 16 * 1024 * 1024;
/// Byte budget for the items of one list page: a page and its envelope stay under one frame.
pub const PAGE_BYTES: usize = MAX_FRAME - 256 * 1024;
/// Upper bound on a requested page length, in items.
pub const MAX_PAGE_ITEMS: u64 = 10_000;
/// List commands that page their items; the window asks for pages, the engine reads whole lists.
const PAGED: [&str; 3] = [
    "qroc-document-list",
    "qroc-document-history",
    "qroc-job-list",
];
const PAGE_FIELDS: [&str; 4] = ["total", "offset", "next_offset", "more"];

/// One page of a list. A caller that names neither `offset` nor `limit` gets every item and no
/// page fields (the engine's own whole-list reads); otherwise the items from `offset`, at most
/// `limit` of them and at most `PAGE_BYTES` (never fewer than one), with `total`, `offset`,
/// `next_offset` and `more` set by `Page::reply`.
pub struct Page {
    items: Vec<Value>,
    fields: Option<Value>,
}
impl Page {
    /// The reply object with the page's items under `key` and, when paged, the page fields.
    pub fn reply(self, key: &str, mut reply: Value) -> Value {
        if let Some(map) = reply.as_object_mut() {
            map.insert(key.to_owned(), Value::Array(self.items));
            if let Some(Value::Object(fields)) = self.fields {
                map.extend(fields);
            }
        }
        reply
    }
}
pub fn page<T: serde::Serialize>(items: &[T], args: &Value) -> Result<Page> {
    let number = |key: &str| -> Result<Option<u64>> {
        match args.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .map(Some)
                .ok_or_else(|| invalid(&format!("Page {key} must be a non-negative integer"))),
        }
    };
    let (offset, limit) = (number("offset")?, number("limit")?);
    if offset.is_none() && limit.is_none() {
        return Ok(Page {
            items: items
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()?,
            fields: None,
        });
    }
    let limit = limit.unwrap_or(MAX_PAGE_ITEMS);
    if limit == 0 || limit > MAX_PAGE_ITEMS {
        return Err(invalid(&format!(
            "Page limit must be between 1 and {MAX_PAGE_ITEMS}"
        )));
    }
    let offset = usize::try_from(offset.unwrap_or(0)).unwrap_or(usize::MAX);
    let mut page = Vec::new();
    let mut bytes = 0_usize;
    for item in items.iter().skip(offset) {
        if page.len() as u64 >= limit {
            break;
        }
        let value = serde_json::to_value(item)?;
        let size = serde_json::to_vec(&value)?.len() + 1;
        if !page.is_empty() && bytes + size > PAGE_BYTES {
            break;
        }
        bytes += size;
        page.push(value);
    }
    let start = offset.min(items.len());
    let next = start + page.len();
    Ok(Page {
        fields: Some(
            json!({"total":items.len(),"offset":start,"next_offset":next,"more":next < items.len()}),
        ),
        items: page,
    })
}
pub struct Bridge {
    qroc: QrocStore,
    phod: PhodStore,
    scope: crate::scope::ScopeStore,
    registry: crate::registry::RegistryStore,
    creative: crate::creative::CreativeStore,
    root: PathBuf,
    seen: BTreeSet<String>,
    hardening: Arc<Mutex<BTreeMap<String, Option<String>>>>,
    tail: Arc<Mutex<BTreeMap<String, Option<String>>>>,
    _lock: fs::File,
}
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
impl Bridge {
    pub fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute()
            || root
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(invalid(
                "Standalone root must be absolute without parent traversal",
            ));
        }
        let mut current = PathBuf::new();
        for part in root.components() {
            current.push(part);
            if let Ok(m) = fs::symlink_metadata(&current)
                && m.file_type().is_symlink()
            {
                return Err(invalid("Standalone root may not contain symlinks"));
            }
        }
        fs::create_dir_all(root)?;
        let root = fs::canonicalize(root)?;
        let lock_path = root.join(".bridge.lock");
        if fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid("Bridge lock may not be a symlink"));
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(lock_path)?;
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn flock(fd: i32, operation: i32) -> i32;
        }
        if unsafe { flock(lock.as_raw_fd(), 2 | 4) } != 0 {
            return Err(invalid(
                "Standalone root is already owned by another bridge",
            ));
        }
        for name in ["qroc", "phod"] {
            if let Ok(m) = fs::symlink_metadata(root.join(name))
                && m.file_type().is_symlink()
            {
                return Err(invalid("Store root may not be a symlink"));
            }
        }
        Ok(Self {
            scope: crate::scope::ScopeStore::open(&root.join("scope"))?,
            registry: crate::registry::RegistryStore::open(&root.join("registry"))?,
            creative: crate::creative::CreativeStore::open(&root.join("creative"))?,
            qroc: QrocStore::open(&root.join("qroc"))?,
            phod: PhodStore::open(&root.join("phod"))?,
            root,
            seen: BTreeSet::new(),
            hardening: Arc::new(Mutex::new(BTreeMap::new())),
            tail: Arc::new(Mutex::new(BTreeMap::new())),
            _lock: lock,
        })
    }
    pub fn ready(&self) -> Value {
        json!({"version":VERSION,"event":"ready","root":self.root,"pid":std::process::id()})
    }
    pub fn reply(&mut self, request: Value) -> Value {
        let id = request
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let result = (|| -> Result<Value> {
            if request.get("version").and_then(Value::as_u64) != Some(VERSION)
                || id.is_empty()
                || id.len() > 128
            {
                return Err(invalid("Invalid bridge version or request ID"));
            }
            if self.seen.len() >= 10000 {
                return Err(invalid("Session request limit reached; restart service"));
            }
            if !self.seen.insert(id.clone()) {
                return Err(invalid("Duplicate request ID"));
            }
            let command = request
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("Command must be a string"))?;
            let mut args = request
                .get("args")
                .filter(|a| a.is_object())
                .ok_or_else(|| invalid("Args must be an object"))?;
            // A window request for a whole list is served its first page; when that page holds
            // every item the reply keeps today's shape, otherwise it carries the page fields.
            let whole = PAGED.contains(&command)
                && ["offset", "limit"].iter().all(|k| args.get(k).is_none());
            let defaulted;
            if whole {
                let mut a = args.clone();
                a["offset"] = json!(0);
                defaulted = a;
                args = &defaulted;
            }
            let mut value = self.route(command, args)?;
            if whole
                && value["more"] == false
                && let Some(map) = value.as_object_mut()
            {
                for k in PAGE_FIELDS {
                    map.remove(k);
                }
            }
            Ok(value)
        })();
        match result {
            Ok(value) => json!({"version":VERSION,"id":id,"ok":true,"result":value,"error":null}),
            Err(e) => {
                json!({"version":VERSION,"id":id,"ok":false,"result":null,"error":e.to_string()})
            }
        }
    }
    fn route(&mut self, command: &str, args: &Value) -> Result<Value> {
        {
            if command == "bridge-prefs-get" {
                let path = self.root.join("prefs.json");
                return Ok(
                    json!({"prefs":if path.exists(){serde_json::from_slice::<Value>(&fs::read(path)?)?}else{json!({})}}),
                );
            }
            if command == "bridge-prefs-save" {
                let prefs = args
                    .get("prefs")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| invalid("Prefs must be an object"))?;
                let bytes = serde_json::to_vec(prefs)?;
                if bytes.len() > 256 * 1024 {
                    return Err(invalid("Prefs exceed 256 KiB"));
                }
                fs::write(self.root.join("prefs.tmp"), bytes)?;
                fs::rename(self.root.join("prefs.tmp"), self.root.join("prefs.json"))?;
                return Ok(json!({"saved":true}));
            }
            if command == "bridge-capabilities" {
                return Ok(
                    json!({"version":VERSION,"scope":true,"registry":true,"creative":self.creative.available(),"generation":"explicit configured provider; discovery is not inference","phod":"source-owned tooling discovery"}),
                );
            }
            if command.starts_with("qroc-scope-") {
                self.scope.command(command, args)
            } else if command.starts_with("qroc-registry-") {
                self.registry.command(command, args)
            } else if command.starts_with("qroc-creative-") {
                if args.get("project_id").is_some() || args.get("branch_id").is_some() {
                    let p = args["project_id"]
                        .as_str()
                        .ok_or_else(|| invalid("Project identity required"))?;
                    let b = args["branch_id"]
                        .as_str()
                        .ok_or_else(|| invalid("Branch identity required"))?;
                    if [p, b].iter().any(|s| {
                        s.is_empty()
                            || s.len() > 200
                            || !s
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    }) {
                        return Err(invalid("Invalid creative owner identity"));
                    }
                    let project: Value = serde_json::from_slice(&fs::read(
                        self.root.join("qroc/projects").join(format!("{p}.json")),
                    )?)?;
                    let branch: Value = serde_json::from_slice(&fs::read(
                        self.root.join("qroc/branches").join(format!("{b}.json")),
                    )?)?;
                    if project["id"] != p || branch["id"] != b || branch["project_id"] != p {
                        return Err(invalid("Creative owner mismatch"));
                    }
                }
                self.creative.command(command, args)
            } else if command == "qroc-media-version-trim" {
                let project = args["project_id"].as_str().unwrap_or("");
                let number = args["version"]
                    .as_u64()
                    .ok_or_else(|| invalid("Version number required"))?;
                let (version, state) = crate::media::version(&self.qroc, project, number)?;
                if state["state"] != "unchanged" {
                    return Err(invalid(
                        "This version's file is changed or missing since it was referenced",
                    ));
                }
                let trim = json!({"start_seconds":args["start_seconds"],"duration_seconds":args["duration_seconds"],"volume":args["volume"]});
                let trim = Value::Object(
                    trim.as_object()
                        .into_iter()
                        .flatten()
                        .filter(|(_, v)| !v.is_null())
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                );
                self.creative.trim_reference(
                    project,
                    version.number,
                    &version.reference,
                    &version.sha256,
                    &trim,
                )
            } else if command.starts_with("qroc-media-") {
                let setting = std::env::var_os(crate::media::MEDIA_ROOT_ENV).map(PathBuf::from);
                crate::media::command(&mut self.qroc, setting.as_deref(), command, args)
            } else if command.starts_with("qroc-harden-") {
                self.harden(command, args)
            } else if command.starts_with("qroc-routes-") {
                routes(&self.root.join("qroc"), command, args)
            } else if command.starts_with("qroc-tail-") {
                self.tail(command, args)
            } else if command.starts_with("qroc-worktable-") {
                crate::worktable::command(&self.root.join("qroc"), command, args)
            } else if command.starts_with("qroc-decide-") {
                crate::decide::command(&self.root, command, args)
            } else if command.starts_with("qroc-") {
                self.qroc.command(command, args)
            } else if command.starts_with("phod-") {
                self.phod.command(command, args)
            } else {
                Err(invalid(
                    "capability-unavailable: command adapter is not implemented",
                ))
            }
        }
    }
    /// Hardening rounds run off the request thread: asking the route for questions may take minutes.
    /// The map holds a project while its round runs (None) and the last failure afterwards (Some).
    fn harden(&mut self, command: &str, args: &Value) -> Result<Value> {
        use crate::harden::{Hardening, ask_questions};
        let project = args
            .get("project_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Project identity required"))?
            .to_owned();
        let qroc_root = self.root.join("qroc");
        let store = Hardening::open(&qroc_root)?;
        let record = store.load(&project)?;
        let state = self
            .hardening
            .lock()
            .map_err(|_| invalid("Hardening state poisoned"))?
            .get(&project)
            .cloned();
        let running = matches!(state, Some(None));
        match command {
            "qroc-harden-get" => {
                Ok(json!({"record":record,"running":running,"error":state.flatten()}))
            }
            "qroc-harden-start" | "qroc-harden-answer" => {
                if running {
                    return Err(invalid("A hardening round is already running"));
                }
                let answers = if command == "qroc-harden-answer" {
                    let text = args
                        .get("answers")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    if text.trim().is_empty() {
                        return Err(invalid("Answers are empty"));
                    }
                    let last = record
                        .rounds
                        .last()
                        .ok_or_else(|| invalid("Hardening not started"))?;
                    if !last.sent_back || last.answers.is_some() {
                        return Err(invalid("The last round takes no answers"));
                    }
                    Some(text)
                } else {
                    if !record.rounds.is_empty() {
                        return Err(invalid("Hardening already started; answer the last round"));
                    }
                    None
                };
                let map = Arc::clone(&self.hardening);
                map.lock()
                    .map_err(|_| invalid("Hardening state poisoned"))?
                    .insert(project.clone(), None);
                std::thread::spawn(move || {
                    let outcome = Hardening::open(&qroc_root).and_then(|h| {
                        let ask = |idea: &str, s: &crate::harden::Score| {
                            ask_questions(&qroc_root, idea, s)
                        };
                        match &answers {
                            Some(a) => h.answer(&project, a, &ask),
                            None => h.start(&project, &ask),
                        }
                    });
                    if let Ok(mut m) = map.lock() {
                        match outcome {
                            Ok(_) => {
                                m.remove(&project);
                            }
                            Err(e) => {
                                m.insert(project, Some(e.to_string()));
                            }
                        }
                    }
                });
                Ok(json!({"running":true}))
            }
            _ => Err(invalid(
                "capability-unavailable: hardening command is not implemented",
            )),
        }
    }
    /// The Product tail (contracts, specs, verification, join) runs off the request thread:
    /// it asks the route twice and may take minutes. Keyed by project and branch, as `harden`.
    fn tail(&mut self, command: &str, args: &Value) -> Result<Value> {
        let project = args
            .get("project_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Project identity required"))?
            .to_owned();
        let branch = args
            .get("branch_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Branch identity required"))?
            .to_owned();
        let qroc_root = self.root.join("qroc");
        let key = format!("{project}/{branch}");
        let state = self
            .tail
            .lock()
            .map_err(|_| invalid("Tail state poisoned"))?
            .get(&key)
            .cloned();
        let running = matches!(state, Some(None));
        match command {
            "qroc-tail-get" => Ok(
                json!({"tail":crate::contracts::tail_state(&qroc_root,&project,&branch)?,"running":running,"error":state.flatten()}),
            ),
            "qroc-tail-start" => {
                if running {
                    return Err(invalid("The tail is already running for this branch"));
                }
                let input = self.qroc.tail_input(&project, &branch)?;
                crate::registry::provider_config_at(&qroc_root)?;
                let map = Arc::clone(&self.tail);
                map.lock()
                    .map_err(|_| invalid("Tail state poisoned"))?
                    .insert(key.clone(), None);
                std::thread::spawn(move || {
                    let outcome = crate::contracts::run_tail(&qroc_root, &input);
                    if let Ok(mut m) = map.lock() {
                        match outcome {
                            Ok(_) => {
                                m.remove(&key);
                            }
                            Err(e) => {
                                m.insert(key, Some(e.to_string()));
                            }
                        }
                    }
                });
                Ok(json!({"running":true}))
            }
            _ => Err(invalid(
                "capability-unavailable: tail command is not implemented",
            )),
        }
    }
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> Result<()> {
        fn emit(output: &mut impl Write, value: &Value) -> Result<()> {
            write(output, serde_json::to_vec(value)?)
        }
        fn write(output: &mut impl Write, bytes: Vec<u8>) -> Result<()> {
            if bytes.len() > MAX_REPLY {
                return Err(invalid(
                    "Bridge reply exceeds 16 MiB; inspect owned export instead",
                ));
            }
            output.write_all(&bytes)?;
            output.write_all(b"\n")?;
            output.flush()?;
            Ok(())
        }
        emit(&mut output, &self.ready())?;
        loop {
            let mut frame = Vec::new();
            let n = input
                .by_ref()
                .take((MAX_FRAME + 1) as u64)
                .read_until(b'\n', &mut frame)?;
            if n == 0 {
                break;
            }
            if n > MAX_FRAME {
                emit(
                    &mut output,
                    &json!({"version":VERSION,"id":null,"ok":false,"result":null,"error":"Frame exceeds 4 MiB; closing service"}),
                )?;
                break;
            }
            let (reply, request) = match serde_json::from_slice::<Value>(&frame) {
                Ok(v) => (self.reply(v.clone()), v),
                Err(e) => (
                    json!({"version":VERSION,"id":null,"ok":false,"result":null,"error":format!("Invalid JSON: {e}")}),
                    Value::Null,
                ),
            };
            let bytes = serde_json::to_vec(&reply)?;
            if bytes.len() > MAX_REPLY {
                emit(&mut output, &oversized(&request, bytes.len()))?;
            } else {
                write(&mut output, bytes)?;
            }
        }
        Ok(())
    }
}

/// The refusal sent in place of a reply over `MAX_REPLY`: it names the command and the page asked
/// for, keeps the request's id, and leaves the service running.
fn oversized(request: &Value, size: usize) -> Value {
    let command = request["command"].as_str().unwrap_or("this command");
    let args = &request["args"];
    let page = match (args["offset"].as_u64(), args["limit"].as_u64()) {
        (None, None) if PAGED.contains(&command) => "its first page".to_owned(),
        (None, None) => "the whole reply".to_owned(),
        (o, Some(l)) => format!("a page of up to {l} items from item {}", o.unwrap_or(0)),
        (Some(o), None) => format!("a page from item {o}"),
    };
    let mib = size as f64 / (1024.0 * 1024.0);
    let hint = if PAGED.contains(&command) {
        "; ask for a smaller page with offset and limit"
    } else {
        ""
    };
    json!({"version":VERSION,"id":request["id"].as_str(),"ok":false,"result":null,
        "error":format!("The reply to {command} for {page} is {mib:.1} MiB, over the bridge's 16 MiB limit{hint}")})
}

/// Route state as read without sending a request: a loopback route is probed by connecting only;
/// an OpenRouter route is ready when its key is held (in the shared layer's store for OpenRouter's own
/// endpoint, in the named environment variable for a loopback stand-in).
fn route_state(r: &crate::routes::Route) -> &'static str {
    use crate::routes::Kind;
    match r.kind {
        Kind::Openrouter => {
            if crate::routes::key_missing(r, &crate::routes::env_var) {
                "key-missing"
            } else {
                "ready"
            }
        }
        Kind::Local | Kind::Relay => {
            let reachable = crate::qroc::parse_local_endpoint(&r.endpoint)
                .ok()
                .filter(|(_, port)| *port != 8080)
                .and_then(|(host, port)| (host.as_str(), port).to_socket_addrs().ok())
                .and_then(|mut a| a.next())
                .is_some_and(|addr| {
                    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
                });
            if reachable { "ready" } else { "unreachable" }
        }
    }
}

/// Routes for the picker: names, kinds and states only; endpoints, models and key references stay in the table.
fn routes(root: &Path, command: &str, args: &Value) -> Result<Value> {
    let mut table = crate::routes::RouteTable::open(root)?;
    if command == "qroc-routes-select" {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Route name required"))?;
        let route = table.get(name).ok_or_else(|| invalid("No such route"))?;
        if route_state(route) != "ready" {
            return Err(invalid("This route is not ready"));
        }
        table.select(name)?;
    } else if command != "qroc-routes-list" {
        return Err(invalid(
            "capability-unavailable: routes command is not implemented",
        ));
    }
    let selected = table.selected().map(|r| r.name.clone());
    Ok(
        json!({"routes":table.list().iter().map(|r| json!({"name":r.name,"kind":r.kind.as_str(),"model":r.model,"state":route_state(r),"selected":Some(&r.name)==selected.as_ref()})).collect::<Vec<_>>()}),
    )
}
