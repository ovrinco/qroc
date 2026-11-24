//! Write-only key broker. Framed JSON over Unix sockets; authenticated encryption on disk.
//! The production front has fixed provider URLs and no caller-supplied headers or redirects.
use crate::{
    Error, Result, chat, images,
    keys::{Redactor, Secret},
};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

const MAX_FRAME: usize = images::REQUEST_LIMIT + 1024 * 1024;
const STORE_AAD: &[u8] = b"stratum-keysvc/1";
const MAX_REPLY: usize = images::REPLY_LIMIT;
/// The longest provider call the service runs: one hour. The legacy transport had no cap, and callers
/// ask for long calls (QROC's shim runs with 15 minutes); a longer request is clamped to this by the
/// client rather than refused, so a long-output call is never stopped before it is sent.
pub const MAX_TIMEOUT_MS: u64 = 3_600_000;
/// The most legacy key-file text an import accepts: such a file is a few lines.
pub const LEGACY_LIMIT: usize = 64 * 1024;

fn fault() -> Error {
    Error::Keys("key service request failed".into())
}
fn at(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::getrandom(&mut bytes).map_err(|_| fault())?;
    Ok(bytes)
}
fn owner(path: &Path, directory: bool) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|e| at(path, e))?;
    let uid = fs::metadata("/proc/self")
        .map_err(|e| at(Path::new("/proc/self"), e))?
        .uid();
    if m.uid() != uid
        || m.mode() & 0o077 != 0
        || m.file_type().is_symlink()
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.nlink() != 1))
    {
        return Err(Error::Keys(
            "key service store ownership or mode is unsafe".into(),
        ));
    }
    Ok(())
}
fn private_read(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    owner(path, false)?;
    let bytes = fs::read(path).map_err(|e| at(path, e))?;
    Ok(Zeroizing::new(bytes))
}
fn private_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| at(path, e))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| at(path, e))
}

struct Record {
    value: Zeroizing<String>,
    last_used: Option<u64>,
}
/// Core state belongs to the service account; it is never shared with a socket caller.
pub struct Store {
    dir: PathBuf,
    master: Zeroizing<Vec<u8>>,
    records: BTreeMap<String, Record>,
    _lock: File,
}
impl Store {
    /// Opens an existing owner-only directory, creating an owner-only master key on first use.
    /// Refuses a missing master for an existing store, unsafe modes, links or damaged ciphertext.
    pub fn open(dir: &Path) -> Result<Self> {
        owner(dir, true)?;
        let lock_path = dir.join("store.lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&lock_path)
            .map_err(|e| at(&lock_path, e))?;
        owner(&lock_path, false)?;
        lock.try_lock()
            .map_err(|_| Error::Keys("key service store is already in use".into()))?;
        let master_path = dir.join("master.key");
        let store_path = dir.join("store.aead");
        if !master_path.try_exists().map_err(|e| at(&master_path, e))? {
            if store_path.try_exists().map_err(|e| at(&store_path, e))? {
                return Err(fault());
            }
            private_new(&master_path, &*Zeroizing::new(random::<32>()?))?;
        }
        let master = private_read(&master_path)?;
        if master.len() != 32 {
            return Err(fault());
        }
        let mut store = Self {
            dir: dir.into(),
            master,
            records: BTreeMap::new(),
            _lock: lock,
        };
        if store_path.try_exists().map_err(|e| at(&store_path, e))? {
            let bytes = private_read(&store_path)?;
            if bytes.len() < 28 {
                return Err(fault());
            }
            let cipher = Aes256Gcm::new_from_slice(&store.master).map_err(|_| fault())?;
            let plain = Zeroizing::new(
                cipher
                    .decrypt(
                        Nonce::from_slice(&bytes[..12]),
                        aes_gcm::aead::Payload {
                            msg: &bytes[12..],
                            aad: STORE_AAD,
                        },
                    )
                    .map_err(|_| fault())?,
            );
            let mut value: Value = serde_json::from_slice(&plain).map_err(|_| fault())?;
            let entries = value.as_object_mut().ok_or_else(fault)?;
            for (name, row) in entries {
                provider(name)?;
                let key = row["value"].as_str().ok_or_else(fault)?.to_owned();
                validate_key(&key)?;
                store.records.insert(
                    name.clone(),
                    Record {
                        value: Zeroizing::new(key),
                        last_used: row["last_used"].as_u64(),
                    },
                );
                if let Some(Value::String(v)) = row.get_mut("value") {
                    zeroize::Zeroize::zeroize(v);
                }
                row["value"] = Value::Null;
            }
        }
        Ok(store)
    }
    /// One-time service-side migration of a protected `legacy.env` inbox, never a socket export.
    /// No key enters caller output.
    pub fn import_legacy(&mut self) -> Result<()> {
        let path = self.dir.join("legacy.env");
        let bytes = private_read(&path)?;
        self.import_legacy_bytes(&bytes)
    }
    /// The same migration from a stream: the privileged bootstrap opens the old key file and hands
    /// its bytes on standard input, so no copy of it is written anywhere and no key is on a command
    /// line. At most [`LEGACY_LIMIT`] bytes are accepted; the buffer is wiped when dropped.
    pub fn import_legacy_from(&mut self, input: impl Read) -> Result<()> {
        // Sized up front so reading never reallocates and leaves an unwiped copy behind.
        let mut bytes = Zeroizing::new(Vec::with_capacity(LEGACY_LIMIT + 64));
        input
            .take(LEGACY_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| fault())?;
        if bytes.len() > LEGACY_LIMIT {
            return Err(fault());
        }
        self.import_legacy_bytes(&bytes)
    }
    fn import_legacy_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        let text = Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(|_| fault())?);
        let mut imported = BTreeMap::new();
        for line in text.lines() {
            let line = line
                .trim()
                .strip_prefix("export ")
                .unwrap_or(line.trim())
                .trim();
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                return Err(fault());
            };
            let name = name.trim();
            if provider(name).is_err() {
                continue;
            }
            if self.records.contains_key(name) || imported.contains_key(name) {
                return Err(fault());
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(value);
            validate_key(value)?;
            imported.insert(
                name.into(),
                Record {
                    value: Zeroizing::new(value.into()),
                    last_used: None,
                },
            );
        }
        if imported.is_empty() {
            return Err(fault());
        }
        let names: Vec<String> = imported.keys().cloned().collect();
        self.records.extend(imported);
        if let Err(e) = self.save() {
            for name in names {
                self.records.remove(&name);
            }
            return Err(e);
        }
        Ok(())
    }
    fn save(&self) -> Result<()> {
        let mut map = serde_json::Map::new();
        for (name, record) in &self.records {
            map.insert(
                name.clone(),
                json!({"value":record.value.as_str(),"last_used":record.last_used}),
            );
        }
        let mut value = Value::Object(map);
        let plain = Zeroizing::new(serde_json::to_vec(&value).map_err(|_| fault())?);
        for row in value.as_object_mut().ok_or_else(fault)?.values_mut() {
            if let Some(Value::String(s)) = row.get_mut("value") {
                zeroize::Zeroize::zeroize(s);
            }
        }
        let nonce = random::<12>()?;
        let cipher = Aes256Gcm::new_from_slice(&self.master).map_err(|_| fault())?;
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: &plain,
                    aad: STORE_AAD,
                },
            )
            .map_err(|_| fault())?;
        let mut bytes = nonce.to_vec();
        bytes.extend(sealed);
        let staged = self.dir.join(format!(
            "store-{}.pending",
            images::b64_encode(&random::<12>()?).replace(['/', '+'], "_")
        ));
        private_new(&staged, &bytes)?;
        let final_path = self.dir.join("store.aead");
        fs::rename(&staged, &final_path).map_err(|e| at(&final_path, e))?;
        File::open(&self.dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| at(&self.dir, e))
    }
    fn status(&self, name: &str) -> Value {
        json!({"ok":true,"set":self.records.contains_key(name),"last_used":self.records.get(name).and_then(|r|r.last_used)})
    }
    fn mutate(&mut self, op: &str, name: &str, value: Option<String>) -> Result<Value> {
        let old = match op {
            "set" | "replace" => {
                let key = Zeroizing::new(value.ok_or_else(fault)?);
                if (op == "set") == self.records.contains_key(name) {
                    return Err(fault());
                }
                validate_key(&key)?;
                self.records.insert(
                    name.into(),
                    Record {
                        value: key,
                        last_used: None,
                    },
                )
            }
            "remove" => self.records.remove(name),
            _ => return Err(fault()),
        };
        if let Err(e) = self.save() {
            match old {
                Some(record) => {
                    self.records.insert(name.into(), record);
                }
                None => {
                    self.records.remove(name);
                }
            }
            return Err(e);
        }
        Ok(self.status(name))
    }
}
fn validate_key(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 4096
        || !value.bytes().all(|c| (33..=126).contains(&c))
        || value.contains(['"', '\\'])
    {
        return Err(fault());
    }
    Ok(())
}
fn provider(name: &str) -> Result<&'static str> {
    match name {
        "OPENAI_API_KEY" => Ok("openai"),
        "ANTHROPIC_API_KEY" => Ok("anthropic"),
        "OPENROUTER_API_KEY" => Ok("openrouter"),
        _ => Err(fault()),
    }
}
fn allowed_url(name: &str, url: &str) -> bool {
    let allowed: &[&str] = match name {
        "OPENAI_API_KEY" => &[
            "https://api.openai.com/v1/chat/completions",
            "https://api.openai.com/v1/responses",
            "https://api.openai.com/v1/images/generations",
            "https://api.openai.com/v1/images/edits",
        ],
        "ANTHROPIC_API_KEY" => &["https://api.anthropic.com/v1/messages"],
        "OPENROUTER_API_KEY" => &[
            "https://openrouter.ai/api/v1/chat/completions",
            "https://openrouter.ai/api/v1/responses",
        ],
        _ => &[],
    };
    allowed.contains(&url)
}
fn frame_write(s: &mut UnixStream, value: &Value) -> Result<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| fault())?);
    if bytes.len() > MAX_FRAME {
        return Err(fault());
    }
    s.write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| s.write_all(&bytes))
        .map_err(|_| fault())
}
fn frame_read(s: &mut UnixStream) -> Result<Value> {
    let mut len = [0; 4];
    s.read_exact(&mut len).map_err(|_| fault())?;
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(fault());
    }
    let mut bytes = Zeroizing::new(vec![0; len]);
    s.read_exact(&mut bytes).map_err(|_| fault())?;
    serde_json::from_slice(&bytes).map_err(|_| fault())
}
fn connect(socket: &Path) -> Result<UnixStream> {
    let s = UnixStream::connect(socket).map_err(|_| fault())?;
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| fault())?;
    s.set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| fault())?;
    Ok(s)
}
/// Management requests; there is deliberately no get/export/sign response operation.
/// Set and replace send a value; replies contain only presence and last-use metadata.
/// The provider keys the service holds, in the order status shows them.
pub const PROVIDER_KEYS: [&str; 3] = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OPENROUTER_API_KEY"];

/// Whether each provider's key is set and when it was last used (seconds since the epoch):
/// metadata only, never a value.
pub fn presence(socket: &Path) -> Result<Vec<(&'static str, bool, Option<u64>)>> {
    PROVIDER_KEYS
        .iter()
        .map(|name| {
            let r = manage(socket, "status", name, None)?;
            Ok((*name, r["set"] == true, r["last_used"].as_u64()))
        })
        .collect()
}

pub fn manage(socket: &Path, op: &str, name: &str, value: Option<&str>) -> Result<Value> {
    let mut s = connect(socket)?;
    frame_write(&mut s, &json!({"op":op,"name":name,"value":value}))?;
    let reply = frame_read(&mut s)?;
    if reply["ok"] == true {
        Ok(reply)
    } else {
        Err(fault())
    }
}
pub(crate) fn require_present(socket: &Path, name: &str) -> Result<()> {
    if manage(socket, "status", name, None)?["set"] == true {
        Ok(())
    } else {
        Err(fault())
    }
}
/// Production daemon loop. The socket must be in a pre-created service-owned runtime directory;
/// its group is inherited from that directory. No unlink/replacement of an existing socket.
pub fn serve(dir: &Path, socket: &Path) -> Result<()> {
    let store = Arc::new(Mutex::new(Store::open(dir)?));
    let runtime = socket.parent().ok_or_else(fault)?;
    let meta = fs::symlink_metadata(runtime).map_err(|e| at(runtime, e))?;
    let uid = fs::metadata("/proc/self")
        .map_err(|e| at(Path::new("/proc/self"), e))?
        .uid();
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o027 != 0 {
        return Err(fault());
    }
    let listener = UnixListener::bind(socket).map_err(|e| at(socket, e))?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o660)).map_err(|e| at(socket, e))?;
    // A bounded pool keeps simultaneous streams independent without unbounded caller threads.
    let (tx, rx) = mpsc::sync_channel::<UnixStream>(16);
    let rx = Arc::new(Mutex::new(rx));
    let mut workers = Vec::new();
    for _ in 0..16 {
        let rx = rx.clone();
        let store = store.clone();
        workers.push(thread::spawn(move || {
            loop {
                let stream = match rx.lock() {
                    Ok(r) => r.recv(),
                    Err(_) => break,
                };
                let Ok(mut stream) = stream else { break };
                let _ = serve_one(&store, &mut stream, &allowed_url);
            }
        }));
    }
    let mut outcome = Ok(());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(mpsc::TrySendError::Full(mut stream)) = tx.try_send(stream) {
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(50)));
                    let _ =
                        frame_write(&mut stream, &json!({"ok":false,"error":"request refused"}));
                }
            }
            Err(e) => {
                outcome = Err(at(socket, e));
                break;
            }
        }
    }
    drop(tx);
    for worker in workers {
        let _ = worker.join();
    }
    outcome
}

fn serve_one(
    store: &Mutex<Store>,
    s: &mut UnixStream,
    policy: &dyn Fn(&str, &str) -> bool,
) -> Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| fault())?;
    s.set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| fault())?;
    let result = dispatch(store, s, policy);
    if result.is_err() {
        let _ = frame_write(s, &json!({"ok":false,"error":"request refused"}));
    }
    result
}
fn dispatch(
    store: &Mutex<Store>,
    s: &mut UnixStream,
    policy: &dyn Fn(&str, &str) -> bool,
) -> Result<()> {
    let mut req = frame_read(s)?;
    let name = req["name"].as_str().ok_or_else(fault)?.to_owned();
    let p = provider(&name)?;
    let op = req["op"].as_str().ok_or_else(fault)?.to_owned();
    match op.as_str() {
        "status" => {
            let reply = store.lock().map_err(|_| fault())?.status(&name);
            frame_write(s, &reply)
        }
        "set" | "replace" | "remove" => {
            let value = if op == "remove" {
                None
            } else {
                req.get_mut("value")
                    .and_then(|v| v.as_str().map(str::to_owned))
            };
            if let Some(Value::String(s)) = req.get_mut("value") {
                zeroize::Zeroize::zeroize(s);
            }
            let reply = store
                .lock()
                .map_err(|_| fault())?
                .mutate(&op, &name, value)?;
            frame_write(s, &reply)
        }
        "call" => {
            let url = req["url"].as_str().ok_or_else(fault)?;
            if !policy(&name, url) {
                return Err(fault());
            }
            let timeout = req["timeout_ms"]
                .as_u64()
                .filter(|t| *t > 0 && *t <= MAX_TIMEOUT_MS)
                .ok_or_else(fault)?;
            let limit = req["limit"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= MAX_REPLY as u64)
                .ok_or_else(fault)? as usize;
            let (key, secrets) = {
                let mut store = store.lock().map_err(|_| fault())?;
                let record = store.records.get_mut(&name).ok_or_else(fault)?;
                let key = Secret::local(record.value.to_string());
                record.last_used = Some(now());
                // Persist the attempt before anything leaves; a failed disk sync sends no request.
                store.save()?;
                let secrets: Vec<_> = store
                    .records
                    .values()
                    .map(|r| r.value.to_string())
                    .map(Zeroizing::new)
                    .collect();
                (key, secrets)
            };
            frame_write(s, &json!({"accepted":true}))?;
            let mut redactor = Redactor::new(secrets);
            let mut gone = false;
            let peer = s.try_clone().map_err(|_| fault())?;
            peer.set_read_timeout(Some(Duration::from_millis(1)))
                .map_err(|_| fault())?;
            let peer = std::cell::RefCell::new(peer);
            let gone_peer = || {
                let mut b = [0; 1];
                match peer.borrow_mut().read_exact(&mut b) {
                    Ok(()) => true,
                    Err(e) => !matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ),
                }
            };
            let ran = chat::post_service_limited(
                url,
                &chat::auth_headers(p, &key),
                &req["body"],
                timeout,
                &gone_peer,
                &mut |chunk| {
                    let out = redactor.push(chunk, false);
                    if !out.is_empty()
                        && frame_write(s, &json!({"chunk":images::b64_encode(&out)})).is_err()
                    {
                        gone = true;
                    }
                    !gone
                },
                limit,
            );
            let tail = redactor.push(&[], true);
            if !tail.is_empty() && !gone {
                frame_write(s, &json!({"chunk":images::b64_encode(&tail)}))?;
            }
            // The broker returns only stable classifications. Curl stderr is discarded at source;
            // no free-form diagnostic can be preprocessed, retained or exposed on this wire.
            frame_write(
                s,
                &json!({"done":true,"status":ran.status,"exit":ran.exit,"reason":ran.reason}),
            )
        }
        _ => Err(fault()),
    }
}

/// Relays provider bytes, preserving streaming, cancellation, HTTP status and transport classification.
#[allow(clippy::too_many_arguments)]
pub fn call(
    socket: &Path,
    name: &str,
    url: &str,
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
    on_chunk: &mut dyn FnMut(&[u8]) -> bool,
    limit: usize,
) -> chat::Ran {
    let mut ran = chat::Ran {
        body: Vec::new(),
        status: None,
        exit: None,
        reason: "spawn failed",
        curl_error: String::new(),
    };
    let timeout_ms = timeout_ms.min(MAX_TIMEOUT_MS);
    let mut run = || -> Result<chat::Ran> {
        let mut s = match connect(socket) {
            Ok(s) => s,
            Err(_) => {
                return Ok(chat::Ran {
                    body: Vec::new(),
                    status: None,
                    exit: None,
                    reason: "key service unreachable",
                    curl_error: String::new(),
                });
            }
        };
        frame_write(
            &mut s,
            &json!({"op":"call","name":name,"url":url,"body":body,"timeout_ms":timeout_ms,"limit":limit}),
        )?;
        // Reads poll cancellation but retain partially read frames across timeouts.
        s.set_read_timeout(Some(Duration::from_millis(50)))
            .map_err(|_| fault())?;
        let start = std::time::Instant::now();
        let mut result = chat::Ran {
            body: Vec::new(),
            status: None,
            exit: None,
            reason: "",
            curl_error: String::new(),
        };
        let mut pending = Vec::new();
        let mut b = [0; 16384];
        let mut accepted = false;
        loop {
            if cancelled()
                || start.elapsed() > Duration::from_millis(timeout_ms.saturating_add(2000))
            {
                result.reason = if cancelled() { "cancelled" } else { "timeout" };
                return Ok(result);
            }
            match s.read(&mut b) {
                Ok(0) => {
                    result.reason = "key service disconnected";
                    return Ok(result);
                }
                Ok(n) => pending.extend_from_slice(&b[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(_) => return Err(fault()),
            }
            while pending.len() >= 4 {
                let n =
                    u32::from_be_bytes([pending[0], pending[1], pending[2], pending[3]]) as usize;
                if n == 0 || n > MAX_FRAME {
                    return Err(fault());
                }
                if pending.len() < n + 4 {
                    break;
                }
                let v: Value = serde_json::from_slice(&pending[4..n + 4]).map_err(|_| fault())?;
                pending.drain(..n + 4);
                if let Some(chunk) = v["chunk"].as_str() {
                    let bytes = images::b64_decode(chunk).ok_or_else(fault)?;
                    if bytes.len() > limit.saturating_sub(result.body.len()) {
                        result.reason = "reply limit";
                        return Ok(result);
                    }
                    result.body.extend_from_slice(&bytes);
                    if !on_chunk(&bytes) {
                        result.reason = "cancelled";
                        return Ok(result);
                    }
                } else if v["accepted"] == true {
                    // From here a broken connection may conceal an outgoing call.
                    accepted = true;
                    continue;
                } else if v["ok"] == false {
                    // Refused before acceptance: the key was not used and nothing was sent.
                    result.reason = if accepted {
                        "key service disconnected"
                    } else {
                        "key service refused"
                    };
                    return Ok(result);
                } else if v["done"] == true {
                    result.status = v["status"].as_u64().and_then(|n| u16::try_from(n).ok());
                    result.exit = v["exit"].as_i64().and_then(|n| i32::try_from(n).ok());
                    // Never accept arbitrary diagnostic text, even from an older/misbehaving peer.
                    result.curl_error.clear();
                    result.reason = match v["reason"].as_str() {
                        Some("") => "",
                        Some("timeout") => "timeout",
                        Some("cancelled") => "cancelled",
                        _ => "nonzero exit",
                    };
                    return Ok(result);
                } else {
                    return Err(fault());
                }
            }
        }
    };
    // A broker disconnect may follow provider transmission: fail conservatively as unknown, never NotSent.
    match run() {
        Ok(r) => r,
        Err(_) => {
            ran.reason = "key service disconnected or refused";
            ran
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Billing, Layer};
    use std::{
        net::{TcpListener, TcpStream},
        sync::atomic::{AtomicBool, Ordering},
        thread::JoinHandle,
    };
    const KEY: &str = "fixture-only-service-SENTINEL-0123456789";

    struct Broker {
        _dir: tempfile::TempDir,
        socket: PathBuf,
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }
    impl Broker {
        fn start(base: Option<String>) -> Self {
            let dir = tempfile::tempdir().unwrap();
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
            let socket = dir.path().join("s");
            let listener = UnixListener::bind(&socket).unwrap();
            let store = Arc::new(Mutex::new(Store::open(dir.path()).unwrap()));
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let join = thread::spawn(move || {
                let mut workers = Vec::new();
                for stream in listener.incoming() {
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let mut stream = stream.unwrap();
                    let store = store.clone();
                    let base = base.clone();
                    workers.push(thread::spawn(move || {
                        let _ = serve_one(&store, &mut stream, &|n, u| {
                            allowed_url(n, u)
                                || base.as_ref().is_some_and(|b| {
                                    u.strip_prefix(b).is_some_and(|p| {
                                        matches!(
                                            p,
                                            "/v1/chat/completions"
                                                | "/v1/responses"
                                                | "/v1/messages"
                                                | "/v1/images/generations"
                                                | "/v1/images/edits"
                                        )
                                    })
                                })
                        });
                    }));
                }
                for worker in workers {
                    worker.join().unwrap();
                }
            });
            Self {
                _dir: dir,
                socket,
                stop,
                join: Some(join),
            }
        }
        fn key(&self, name: &str) {
            manage(&self.socket, "set", name, Some(KEY)).unwrap();
        }
    }
    impl Drop for Broker {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = UnixStream::connect(&self.socket);
            self.join.take().unwrap().join().unwrap();
        }
    }
    type Seen = Arc<Mutex<Vec<(String, Value)>>>;
    struct Fake {
        base: String,
        seen: Seen,
        reply: Arc<Mutex<(u16, Vec<u8>)>>,
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }
    impl Fake {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let seen: Seen = Arc::default();
            let got = seen.clone();
            let reply = Arc::new(Mutex::new((200, b"{}".to_vec())));
            let sent = reply.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let join = thread::spawn(move || {
                for conn in listener.incoming() {
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    let mut conn = conn.unwrap();
                    conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    let mut data = Vec::new();
                    let mut b = [0; 8192];
                    let head_end = loop {
                        let n = conn.read(&mut b).unwrap();
                        assert!(n > 0);
                        data.extend_from_slice(&b[..n]);
                        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&data[..head_end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if head.to_lowercase().contains("expect: 100-continue") {
                        conn.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
                    }
                    while data.len() < head_end + length {
                        let n = conn.read(&mut b).unwrap();
                        assert!(n > 0);
                        data.extend_from_slice(&b[..n]);
                    }
                    got.lock().unwrap().push((
                        head,
                        serde_json::from_slice(&data[head_end..head_end + length]).unwrap(),
                    ));
                    let (status, body) = sent.lock().unwrap().clone();
                    let _=conn.write_all(format!("HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",body.len()).as_bytes());
                    // Deliberately split keys and SSE events across writes.
                    for chunk in body.chunks(11) {
                        if conn.write_all(chunk).is_err() {
                            break;
                        }
                    }
                }
            });
            Self {
                base,
                seen,
                reply,
                stop,
                join: Some(join),
            }
        }
        fn reply(&self, status: u16, body: &[u8]) {
            *self.reply.lock().unwrap() = (status, body.into());
        }
        fn hits(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
            self.join.take().unwrap().join().unwrap();
        }
    }
    fn dir() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        d
    }
    fn raw(socket: &Path, req: Value) -> Value {
        let mut s = connect(socket).unwrap();
        frame_write(&mut s, &req).unwrap();
        frame_read(&mut s).unwrap()
    }
    fn layer(d: &Path, broker: &Broker, base: &str) -> Layer {
        let config = d.join("config");
        fs::create_dir(&config).unwrap();
        fs::write(
            config.join("keysvc.json"),
            json!({"socket":broker.socket}).to_string(),
        )
        .unwrap();
        // If any path tries to read the legacy store, opening this directory as a file fails.
        fs::create_dir(config.join("keys.env")).unwrap();
        fs::write(config.join("prices.json"),json!({"schema":"qroc-prices/1","currency":"USD","models":{
            "openai:fake":{"provider":"openai","model":"fake","input":"2","cached_input":"1","output":"10","source_url":"fixture","read_on":"2026-10-30"},
            "anthropic:fake":{"provider":"anthropic","model":"fake","input":"4","cache_write_5m":"5","cache_write_1h":"8","cached_input":"0.2","output":"20","source_url":"fixture","read_on":"2026-10-30"},
            "openrouter:fake":{"provider":"openrouter","model":"fake","input":"2","cached_input":"1","output":"10","source_url":"fixture","read_on":"2026-10-30"},
            "openai:image":{"provider":"openai","model":"image","kind":"image","input":"5","cached_input":"1","image_input":"8","cached_image_input":"2","output":"30","image_output_base":{"low":16},"input_tokens_per_image_bound":1000,"source_url":"fixture","read_on":"2026-10-30"}
        }}).to_string()).unwrap();
        fs::write(config.join("caps.json"),json!({"schema":"stratum-caps/1","currency":"USD","tools":{"test":{"total":"1","per_day":"1"}}}).to_string()).unwrap();
        fs::write(config.join("routes.json"),json!({"routes":{
            "openai:fake":{"provider":"openai","model":"fake","base":base,"max_output_tokens":100},
            "responses:openai":{"provider":"openai","model":"fake","base":base,"max_output_tokens":100},
            "responses:anthropic":{"provider":"anthropic","model":"fake","base":base,"max_output_tokens":100},
            "responses:openrouter":{"provider":"openrouter","model":"fake","base":base,"max_output_tokens":100},
            "images:openai":{"provider":"openai","model":"image","base":base,"max_output_tokens":1000}
        }}).to_string()).unwrap();
        Layer::at(d.join("home"), config)
    }

    #[test]
    fn encrypted_store_survives_restart_and_rejects_tampering_modes_and_links() {
        let d = dir();
        let mut store = Store::open(d.path()).unwrap();
        assert!(Store::open(d.path()).is_err(), "single writer lock");
        store
            .mutate("set", "OPENAI_API_KEY", Some(KEY.into()))
            .unwrap();
        let sealed = fs::read(d.path().join("store.aead")).unwrap();
        assert!(!sealed.windows(KEY.len()).any(|w| w == KEY.as_bytes()));
        store.save().unwrap();
        assert_ne!(
            sealed,
            fs::read(d.path().join("store.aead")).unwrap(),
            "fresh nonce"
        );
        drop(store);
        let store = Store::open(d.path()).unwrap();
        assert_eq!(store.records["OPENAI_API_KEY"].value.as_str(), KEY);
        drop(store);
        let p = d.path().join("store.aead");
        let mut bad = fs::read(&p).unwrap();
        bad[15] ^= 1;
        fs::write(&p, &bad).unwrap();
        assert!(Store::open(d.path()).is_err());
        fs::write(&p, &sealed).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Store::open(d.path()).is_err());
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let original = d.path().join("original");
        fs::rename(&p, &original).unwrap();
        std::os::unix::fs::symlink(&original, &p).unwrap();
        assert!(Store::open(d.path()).is_err());
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Store::open(d.path()).is_err());
    }
    #[test]
    fn wrong_or_missing_master_never_silently_reinitializes() {
        let d = dir();
        let mut store = Store::open(d.path()).unwrap();
        store
            .mutate("set", "OPENAI_API_KEY", Some(KEY.into()))
            .unwrap();
        drop(store);
        let key = d.path().join("master.key");
        fs::rename(&key, d.path().join("kept-master")).unwrap();
        assert!(Store::open(d.path()).is_err());
        assert!(!key.exists());
        private_new(&key, &[7; 32]).unwrap();
        assert!(Store::open(d.path()).is_err());
    }
    #[test]
    fn socket_management_has_no_readback_and_replacement_is_explicit() {
        let broker = Broker::start(None);
        assert_eq!(
            manage(&broker.socket, "status", "OPENAI_API_KEY", None).unwrap()["set"],
            false
        );
        assert!(manage(&broker.socket, "replace", "OPENAI_API_KEY", Some(KEY)).is_err());
        broker.key("OPENAI_API_KEY");
        assert!(manage(&broker.socket, "set", "OPENAI_API_KEY", Some(KEY)).is_err());
        let metadata = manage(
            &broker.socket,
            "replace",
            "OPENAI_API_KEY",
            Some("fixture-replaced"),
        )
        .unwrap();
        assert_eq!(metadata["set"], true);
        assert!(metadata["last_used"].is_null());
        assert!(!metadata.to_string().contains("fixture"));
        for op in ["get", "export", "sign", "unknown"] {
            assert!(manage(&broker.socket, op, "OPENAI_API_KEY", None).is_err());
        }
        assert!(manage(&broker.socket, "set", "OTHER", Some(KEY)).is_err());
        for key in ["", "key\nInjected: yes", "a\"b", "a\\b"] {
            assert!(manage(&broker.socket, "replace", "OPENAI_API_KEY", Some(key)).is_err());
        }
        assert_eq!(
            manage(&broker.socket, "remove", "OPENAI_API_KEY", None).unwrap()["set"],
            false
        );
        assert_eq!(
            manage(&broker.socket, "remove", "OPENAI_API_KEY", None).unwrap()["set"],
            false
        );
        let mut stream = connect(&broker.socket).unwrap();
        stream.write_all(&u32::MAX.to_be_bytes()).unwrap();
        assert_eq!(frame_read(&mut stream).unwrap()["ok"], false);
    }
    #[test]
    fn destinations_are_exact_and_provider_specific() {
        for url in [
            "http://127.0.0.1:9000/v1/responses",
            "https://api.openai.com.evil/v1/responses",
            "https://api.openai.com@evil/v1/responses",
            "https://api.openai.com/v1/responses?next=evil",
            "file:///etc/passwd",
            "https://api.openai.com/v1/../responses",
            "https://api.openai.com:443/v1/responses",
        ] {
            assert!(!allowed_url("OPENAI_API_KEY", url), "{url}");
        }
        assert!(allowed_url(
            "OPENAI_API_KEY",
            "https://api.openai.com/v1/responses"
        ));
        assert!(!allowed_url(
            "ANTHROPIC_API_KEY",
            "https://api.openai.com/v1/responses"
        ));
        assert!(allowed_url(
            "OPENROUTER_API_KEY",
            "https://openrouter.ai/api/v1/chat/completions"
        ));
        let broker = Broker::start(None);
        broker.key("OPENAI_API_KEY");
        assert_eq!(
            raw(
                &broker.socket,
                json!({"op":"call","name":"OPENAI_API_KEY","url":"http://127.0.0.1:9/v1/responses","body":{},"timeout_ms":10,"limit":100})
            )["ok"],
            false
        );
        assert!(
            manage(&broker.socket, "status", "OPENAI_API_KEY", None).unwrap()["last_used"]
                .is_null()
        );
    }
    #[test]
    fn streaming_redaction_handles_every_split_and_all_keys() {
        let text = format!("start-{KEY}-mid-other-secret-finish");
        for split in 0..=text.len() {
            let mut r = Redactor::new(vec![
                Zeroizing::new(KEY.into()),
                Zeroizing::new("other-secret".into()),
            ]);
            let mut out = r.push(&text.as_bytes()[..split], false);
            out.extend(r.push(&text.as_bytes()[split..], true));
            assert_eq!(out, b"start-***-mid-***-finish");
        }
    }
    fn redact_chunks(keys: &[&str], text: &[u8], cuts: &[usize]) -> Vec<u8> {
        let mut r = Redactor::new(keys.iter().map(|k| Zeroizing::new((*k).into())).collect());
        let mut out = Vec::new();
        let mut from = 0;
        for &to in cuts {
            out.extend(r.push(&text[from..to], false));
            from = to;
        }
        out.extend(r.push(&text[from..], true));
        out
    }
    fn assert_every_split(keys: &[&str], text: &str, expected: &[u8]) {
        for first in 0..=text.len() {
            for second in first..=text.len() {
                assert_eq!(
                    redact_chunks(keys, text.as_bytes(), &[first, second]),
                    expected,
                    "keys {keys:?}, cuts {first},{second}"
                );
            }
        }
        assert_eq!(
            redact_chunks(keys, text.as_bytes(), &(0..=text.len()).collect::<Vec<_>>()),
            expected
        );
    }
    #[test]
    fn overlapping_keys_mask_the_union_independent_of_order_and_every_split() {
        for (keys, text, expected) in [
            (
                vec!["sk-", "sk-fixture-unknown-SENTINEL"],
                "sk-fixture-unknown-SENTINEL",
                "***",
            ),
            (vec!["abcde", "cdefg", "efghi"], "abcdefghij", "***j"),
            (vec!["ab", "bc"], "abc", "***"),
            (vec!["ababa"], "abababa", "***"),
            (vec!["aaab", "aabx"], "aaabx", "***"),
            (vec!["abcdef", "cd", "abcdef"], "[abcdef]", "[***]"),
        ] {
            assert_every_split(&keys, text, expected.as_bytes());
            let reversed = keys.into_iter().rev().collect::<Vec<_>>();
            assert_every_split(&reversed, text, expected.as_bytes());
        }
    }
    #[test]
    fn truncated_credential_prefix_is_masked_at_end_instead_of_flushed() {
        let key = "sk-fixture-unknown-SENTINEL";
        for length in 1..key.len() {
            let text = format!("[{prefix}", prefix = &key[..length]);
            assert_every_split(&["sk-", key], &text, b"[***");
            assert_every_split(&[key, "sk-"], &text, b"[***");
        }
        assert_every_split(&["abcde", "cdefghi"], "abcdef", b"***");
        assert_every_split(&["abcde", "cdefghi"], "abcdefghi!", b"***!");
    }
    #[test]
    fn body_then_diagnostic_cannot_disclose_an_overlapping_key_suffix() {
        let key = "sk-fixture-unknown-SENTINEL";
        let mut r = Redactor::new(vec![
            Zeroizing::new("sk-".into()),
            Zeroizing::new(key.into()),
        ]);
        assert_eq!(
            r.push(format!("body {key}!").as_bytes(), true),
            b"body ***!"
        );
        assert_eq!(
            r.push(format!("curl said {key}!").as_bytes(), true),
            b"curl said ***!"
        );
    }
    #[test]
    fn redaction_matches_independent_interval_oracle_for_all_small_binary_inputs() {
        let key_sets = [
            vec!["a", "ab", "ba"],
            vec!["aba", "bab"],
            vec!["aaab", "aab"],
            vec!["bba", "baa", "aaa"],
        ];
        for keys in key_sets {
            for length in 0..=8 {
                for bits in 0..(1usize << length) {
                    let text = (0..length)
                        .map(|i| if bits & (1 << i) == 0 { b'a' } else { b'b' })
                        .collect::<Vec<_>>();
                    let mut hidden = vec![false; text.len()];
                    for start in 0..text.len() {
                        for key in &keys {
                            if text[start..].starts_with(key.as_bytes()) {
                                hidden[start..start + key.len()].fill(true);
                            } else if key.as_bytes().starts_with(&text[start..]) {
                                hidden[start..].fill(true);
                            }
                        }
                    }
                    let mut expected = Vec::new();
                    let mut inside = false;
                    for (&byte, &hide) in text.iter().zip(&hidden) {
                        if hide {
                            if !inside {
                                expected.extend_from_slice(b"***");
                            }
                        } else {
                            expected.push(byte);
                        }
                        inside = hide;
                    }
                    for split in 0..=text.len() {
                        assert_eq!(
                            redact_chunks(&keys, &text, &[split]),
                            expected,
                            "keys {keys:?}, text {text:?}, split {split}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn broker_client_ignores_free_form_diagnostics_and_unrecognized_reason_text() {
        let dir = dir();
        let socket = dir.path().join("peer.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let peer = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _ = frame_read(&mut s).unwrap();
            frame_write(&mut s, &json!({"accepted":true})).unwrap();
            frame_write(
                &mut s,
                &json!({"done":true,"status":502,"exit":2,
                "reason":"sk-fixture-PREFIX!TAIL-SENTINEL","curl_error":"***!TAIL-SENTINEL"}),
            )
            .unwrap();
        });
        let ran = call(
            &socket,
            "OPENAI_API_KEY",
            "https://api.openai.com/v1/responses",
            &json!({}),
            1000,
            &|| false,
            &mut |_| true,
            1000,
        );
        peer.join().unwrap();
        assert!(ran.curl_error.is_empty());
        assert_eq!(ran.reason, "nonzero exit");
        assert!(!format!("{ran:?}").contains("SENTINEL"));
    }
    #[test]
    fn broker_accepts_long_caller_timeouts_and_names_its_own_refusals() {
        // QROC's shim runs with a 15-minute timeout; the legacy transport had no cap. The service took
        // up to 10 minutes and refused anything longer before sending, which the client reported as
        // "curl did not start" (31 Oct, receipt 1793472751712-3256424-0).
        let fake = Fake::start();
        let broker = Broker::start(Some(fake.base.clone()));
        broker.key("ANTHROPIC_API_KEY");
        let url = format!("{}/v1/messages", fake.base);
        for timeout in [900_000, MAX_TIMEOUT_MS, MAX_TIMEOUT_MS + 1, u64::MAX] {
            fake.reply(200, b"{}");
            let ran = call(
                &broker.socket,
                "ANTHROPIC_API_KEY",
                &url,
                &json!({"max_tokens":128000,"messages":[]}),
                timeout,
                &|| false,
                &mut |_| true,
                chat::BODY_LIMIT,
            );
            assert_eq!(ran.reason, "", "timeout {timeout}: {ran:?}");
            assert_eq!(ran.status, Some(200), "timeout {timeout}");
        }
        // A refusal before acceptance is named as one, not as a transport that never started.
        let ran = call(
            &broker.socket,
            "ANTHROPIC_API_KEY",
            "https://example.invalid/v1/messages",
            &json!({}),
            1000,
            &|| false,
            &mut |_| true,
            1000,
        );
        assert_eq!(ran.reason, "key service refused");
        let gone = broker.socket.with_file_name("absent.sock");
        let ran = call(
            &gone,
            "ANTHROPIC_API_KEY",
            &url,
            &json!({}),
            1000,
            &|| false,
            &mut |_| true,
            1000,
        );
        assert_eq!(ran.reason, "key service unreachable");
    }
    #[test]
    fn broker_call_streams_masks_and_records_last_attempt() {
        let fake = Fake::start();
        let broker = Broker::start(Some(fake.base.clone()));
        broker.key("OPENAI_API_KEY");
        manage(&broker.socket, "set", "ANTHROPIC_API_KEY", Some("fixture-")).unwrap();
        fake.reply(200, format!("data: {KEY}\n\ndata: [DONE]\n\n").as_bytes());
        let mut forwarded = Vec::new();
        let ran = call(
            &broker.socket,
            "OPENAI_API_KEY",
            &format!("{}/v1/chat/completions", fake.base),
            &json!({"messages":[]}),
            2000,
            &|| false,
            &mut |b| {
                forwarded.extend_from_slice(b);
                true
            },
            10000,
        );
        assert_eq!(ran.status, Some(200));
        assert_eq!(ran.reason, "");
        assert_eq!(ran.body, forwarded);
        assert_eq!(ran.body, b"data: ***\n\ndata: [DONE]\n\n");
        assert!(!String::from_utf8_lossy(&ran.body).contains(KEY));
        assert!(
            fake.seen.lock().unwrap()[0]
                .0
                .contains(&format!("Authorization: Bearer {KEY}"))
        );
        assert!(
            manage(&broker.socket, "status", "OPENAI_API_KEY", None).unwrap()["last_used"].is_u64()
        );
        fake.reply(401, format!("{{\"error\":\"{KEY}\"}}").as_bytes());
        let ran = call(
            &broker.socket,
            "OPENAI_API_KEY",
            &format!("{}/v1/responses", fake.base),
            &json!({}),
            2000,
            &|| false,
            &mut |_| true,
            10000,
        );
        assert_eq!(ran.status, Some(401));
        assert!(!String::from_utf8_lossy(&ran.body).contains(KEY));
    }
    #[test]
    fn isolated_transport_never_returns_raw_provider_body_in_a_debuggable_result() {
        let fake = Fake::start();
        let value = "sk-fixture-PREFIX!TAIL-SENTINEL";
        fake.reply(200, format!("response {value}!").as_bytes());
        let key = Secret::local(value.into());
        let mut filter = Redactor::new(vec![Zeroizing::new(value.into())]);
        let mut forwarded = Vec::new();
        let result = chat::post_service_limited(
            &format!("{}/v1/responses", fake.base),
            &chat::auth_headers("openai", &key),
            &json!({}),
            1000,
            &|| false,
            &mut |bytes| {
                forwarded.extend(filter.push(bytes, false));
                true
            },
            1000,
        );
        forwarded.extend(filter.push(&[], true));
        assert_eq!(forwarded, b"response ***!");
        assert_eq!(result.status, Some(200));
        assert!(result.body.is_empty());
        assert!(result.curl_error.is_empty());
        assert!(!format!("{result:?}").contains("SENTINEL"));
    }
    #[test]
    fn broker_failure_body_error_and_ledger_never_retain_punctuation_key_material() {
        let fake = Fake::start();
        let broker = Broker::start(Some(fake.base.clone()));
        let key = "sk-fixture-PREFIX!TAIL-SENTINEL";
        manage(&broker.socket, "set", "OPENAI_API_KEY", Some(key)).unwrap();
        let d = dir();
        let layer = layer(d.path(), &broker, &fake.base);
        fake.reply(
            401,
            json!({"error":{"message":key,"type":key,"code":key}})
                .to_string()
                .as_bytes(),
        );
        let failure = images::run(
            &layer,
            "test",
            images::Kind::Generate,
            &json!({"prompt":"fixture","size":"1024x640","quality":"low"}),
            1000,
            &|| false,
        )
        .unwrap_err();
        assert_eq!(
            failure.detail["provider_message"],
            "the provider call failed"
        );
        assert!(!format!("{failure:?}").contains("TAIL-SENTINEL"));
        let ledger = fs::read_to_string(layer.home.join("ledger.jsonl")).unwrap();
        assert!(!ledger.contains("TAIL-SENTINEL"));
        for file in fs::read_dir(layer.home.join("failed")).unwrap() {
            assert!(
                !String::from_utf8_lossy(&fs::read(file.unwrap().path()).unwrap())
                    .contains("TAIL-SENTINEL")
            );
        }
        fake.reply(
            200,
            format!("data: {{\"error\":{{\"message\":\"{key}\"}}}}\n\n").as_bytes(),
        );
        let outcome = chat::complete(
            &layer,
            &chat::Call {
                id: "stream-error",
                provider: "openai",
                base: &fake.base,
                model: "fake",
                effort: None,
                messages: &json!([]),
                max_output: 10,
                timeout_ms: 1000,
                keep_dir: None,
            },
            &|| false,
        );
        assert!(outcome.reply.is_err());
        assert!(!format!("{:?}", outcome.billing).contains("TAIL-SENTINEL"));
    }
    #[test]
    fn switched_layer_never_reads_legacy_even_if_service_fails() {
        let fake = Fake::start();
        let broker = Broker::start(Some(fake.base.clone()));
        broker.key("OPENAI_API_KEY");
        let d = dir();
        let layer = layer(d.path(), &broker, &fake.base);
        assert!(layer.has_key("OPENAI_API_KEY"));
        assert_eq!(layer.key("OPENAI_API_KEY").unwrap().expose(), "");
        let messages = json!([{"role":"user","content":"hello"}]);
        fake.reply(200,b"{\"id\":\"fixture\",\"model\":\"fake\",\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}");
        let result = chat::complete(
            &layer,
            &chat::Call {
                id: "fixture",
                provider: "openai",
                base: &fake.base,
                model: "fake",
                effort: None,
                messages: &messages,
                max_output: 10,
                timeout_ms: 2000,
                keep_dir: None,
            },
            &|| false,
        );
        assert!(result.reply.is_ok());
        assert!(matches!(result.billing, Billing::Usage { .. }));
        assert_eq!(fake.hits(), 1);
        drop(broker);
        assert!(!layer.has_key("OPENAI_API_KEY"));
        assert!(layer.key("OPENAI_API_KEY").is_err());
        fs::write(layer.config.join("keysvc.json"), "malformed").unwrap();
        assert!(!layer.has_key("OPENAI_API_KEY"));
        assert!(layer.key("OPENAI_API_KEY").is_err());
    }
    fn responses_request(layer: &Layer, provider: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let layer = layer.clone();
        let body =
            json!({"model":provider,"input":[{"role":"user","content":"hello"}],"stream":true})
                .to_string();
        let worker = thread::spawn(move || {
            crate::responses::handle(&layer, "test", body.as_bytes(), &mut server, 2000)
        });
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        worker.join().unwrap();
        reply
    }
    fn events(events: &[Value]) -> String {
        events
            .iter()
            .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
            .collect()
    }
    #[test]
    fn responses_and_anthropic_translation_keep_streams_usage_and_caps() {
        for provider in ["openai", "anthropic", "openrouter"] {
            let fake = Fake::start();
            let broker = Broker::start(Some(fake.base.clone()));
            let name = match provider {
                "anthropic" => "ANTHROPIC_API_KEY",
                "openrouter" => "OPENROUTER_API_KEY",
                _ => "OPENAI_API_KEY",
            };
            broker.key(name);
            let d = dir();
            let layer = layer(d.path(), &broker, &fake.base);
            let output = if provider == "anthropic" {
                events(&[
                    json!({"type":"message_start","message":{"id":"fixture","model":"fake","content":[],"usage":{"input_tokens":3,"output_tokens":1}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
                    json!({"type":"message_stop"}),
                ])
            } else {
                events(&[
                    json!({"type":"response.created","response":{"id":"fixture","status":"in_progress","model":"fake"}}),
                    json!({"type":"response.output_text.delta","delta":"hello","output_index":0,"content_index":0}),
                    json!({"type":"response.completed","response":{"id":"fixture","status":"completed","model":"fake","usage":{"input_tokens":3,"output_tokens":2}}}),
                ])
            };
            fake.reply(200, output.as_bytes());
            let reply = responses_request(&layer, provider);
            assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
            assert!(reply.contains("response.completed"));
            assert!(reply.contains("stratum-api receipt"));
            let got = fake.seen.lock().unwrap();
            if provider == "anthropic" {
                assert!(got[0].0.starts_with("POST /v1/messages"));
                assert!(got[0].0.contains(&format!("x-api-key: {KEY}")));
            } else {
                assert!(got[0].0.starts_with("POST /v1/responses"));
                assert!(got[0].0.contains(&format!("Authorization: Bearer {KEY}")));
            }
            drop(got);
            let ledger = fs::read_to_string(layer.home.join("ledger.jsonl")).unwrap();
            assert!(ledger.contains("settle"));
            assert!(!ledger.contains(KEY));
            fs::write(layer.caps_path(),json!({"schema":"stratum-caps/1","currency":"USD","tools":{"test":{"total":"0.000001"}}}).to_string()).unwrap();
            assert!(responses_request(&layer, provider).starts_with("HTTP/1.1 429"));
            assert_eq!(fake.hits(), 1);
        }
    }

    fn silent_provider() -> (String, mpsc::Receiver<()>, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut b = [0; 8192];
            assert!(conn.read(&mut b).unwrap() > 0);
            tx.send(()).unwrap();
            // The fixture never replies. Cancellation/timeout must close the owned curl connection.
            while conn.read(&mut b).unwrap() != 0 {}
        });
        (base, rx, worker)
    }
    #[test]
    fn silent_call_allows_concurrent_status_and_cancels_without_leftovers() {
        let (base, started, provider) = silent_provider();
        let broker = Broker::start(Some(base.clone()));
        broker.key("OPENAI_API_KEY");
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let socket = broker.socket.clone();
        let caller = thread::spawn(move || {
            call(
                &socket,
                "OPENAI_API_KEY",
                &format!("{base}/v1/responses"),
                &json!({}),
                2000,
                &|| flag.load(Ordering::SeqCst),
                &mut |_| true,
                1000,
            )
        });
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        let start = std::time::Instant::now();
        assert_eq!(
            manage(&broker.socket, "status", "OPENAI_API_KEY", None).unwrap()["set"],
            true
        );
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "status waits on no network mutex"
        );
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(caller.join().unwrap().reason, "cancelled");
        provider.join().unwrap();
        drop(broker);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "curl is cancelled and reaped"
        );
    }
    #[test]
    fn broker_timeout_preserves_transport_failure_and_reaps_curl() {
        let (base, started, provider) = silent_provider();
        let broker = Broker::start(Some(base.clone()));
        broker.key("OPENAI_API_KEY");
        let ran = call(
            &broker.socket,
            "OPENAI_API_KEY",
            &format!("{base}/v1/responses"),
            &json!({}),
            100,
            &|| false,
            &mut |_| true,
            1000,
        );
        started.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(ran.status, None);
        assert_eq!(ran.exit, Some(28));
        assert!(!ran.reason.is_empty());
        assert!(!ran.curl_error.contains(KEY));
        provider.join().unwrap();
        drop(broker);
    }

    #[test]
    fn images_keep_cap_reservation_settlement_and_refusal_through_broker() {
        let fake = Fake::start();
        let broker = Broker::start(Some(fake.base.clone()));
        broker.key("OPENAI_API_KEY");
        let d = dir();
        let layer = layer(d.path(), &broker, &fake.base);
        fake.reply(200,br#"{"created":1,"data":[{"b64_json":"iVBORw0KGgo="}],"usage":{"input_tokens":20,"input_tokens_details":{"text_tokens":20,"image_tokens":0},"output_tokens":107}}"#);
        let request = json!({"prompt":"fixture","size":"1024x640","quality":"low"});
        let (_, receipt) = images::run(
            &layer,
            "test",
            images::Kind::Generate,
            &request,
            2000,
            &|| false,
        )
        .unwrap();
        assert_eq!(receipt.charge, Some(3_310_000));
        assert_eq!(fake.hits(), 1);
        fs::write(layer.caps_path(),json!({"schema":"stratum-caps/1","currency":"USD","tools":{"test":{"total":"0.000001"}}}).to_string()).unwrap();
        assert!(
            images::run(
                &layer,
                "test",
                images::Kind::Generate,
                &request,
                2000,
                &|| false
            )
            .is_err()
        );
        assert_eq!(fake.hits(), 1);
        let ledger = fs::read_to_string(layer.home.join("ledger.jsonl")).unwrap();
        assert!(ledger.contains("admit"));
        assert!(ledger.contains("settle"));
        assert!(!ledger.contains(KEY));
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn migration_only_reads_an_owner_only_inbox_and_never_returns_keys() {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        private_new(
            &dir.path().join("legacy.env"),
            b"OPENAI_API_KEY=fixture-import-only\n",
        )
        .unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.import_legacy().unwrap();
        assert_eq!(store.status("OPENAI_API_KEY")["set"], true);
        assert_eq!(
            store.records["OPENAI_API_KEY"].value.as_str(),
            "fixture-import-only"
        );
        assert!(store.import_legacy().is_err());
    }
    #[test]
    fn migration_from_a_stream_stores_keys_and_refuses_duplicates_bad_text_and_oversize() {
        let dir = tempfile::tempdir().unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store
            .import_legacy_from(
                &b"# kept\nexport OPENAI_API_KEY=\"fixture-stream-only\"\nOTHER=x\n"[..],
            )
            .unwrap();
        assert_eq!(
            store.records["OPENAI_API_KEY"].value.as_str(),
            "fixture-stream-only"
        );
        assert!(!dir.path().join("legacy.env").exists());
        assert!(
            store
                .import_legacy_from(&b"OPENAI_API_KEY=fixture-again"[..])
                .is_err()
        );
        drop(store);
        let mut store = Store::open(dir.path()).unwrap();
        assert_eq!(store.status("OPENAI_API_KEY")["set"], true);
        assert!(store.import_legacy_from(&b"no equals sign"[..]).is_err());
        assert!(store.import_legacy_from(&b""[..]).is_err());
        let mut big = b"ANTHROPIC_API_KEY=fixture-big\n".to_vec();
        big.resize(LEGACY_LIMIT + 1, b'#');
        assert!(store.import_legacy_from(&big[..]).is_err());
        assert_eq!(store.status("ANTHROPIC_API_KEY")["set"], false);
    }
}
