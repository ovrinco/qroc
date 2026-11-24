//! Routes table kept by the app: which brain a brief goes to.
//! Launch variables (QROC_PROVIDER, QROC_GATEWAY, QROC_MODEL) only seed an absent table.
//! A route to OpenRouter's own endpoint is sent through the shared API layer (Stratum), which holds the
//! key in its one store, prices the call and checks it against QROC's cap: the app keeps no key for it.
//! Only a loopback stand-in for that endpoint names a key by an environment variable, never stored,
//! logged or printed.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub const TABLE_VERSION: &str = "qroc-routes/1";
pub const OPENROUTER_ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";
const MASK: &str = "Bearer ****";
/// The shared API layer's local service; QROC names itself in the path.
pub const LAYER_CHAT: &str = "http://127.0.0.1:8098/tool/qroc/v1/chat/completions";

/// The layer's chat address: `STRATUM_API_URL` (a loopback `http://127.0.0.1:<port>`, for tests and
/// proofs only) or the installed service.
fn layer_chat(env: &dyn Fn(&str) -> Option<String>) -> String {
    env("STRATUM_API_URL")
        .filter(|u| loopback_http(u))
        .map(|u| format!("{}/tool/qroc/v1/chat/completions", u.trim_end_matches('/')))
        .unwrap_or_else(|| LAYER_CHAT.into())
}
/// The key the layer holds for OpenRouter (a name, never a value).
pub const LAYER_OPENROUTER_KEY: &str = "OPENROUTER_API_KEY";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Local,
    Relay,
    Openrouter,
}
impl Kind {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "local" => Ok(Kind::Local),
            "relay" => Ok(Kind::Relay),
            "openrouter" => Ok(Kind::Openrouter),
            _ => Err(invalid("route kind must be local, relay or openrouter")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Local => "local",
            Kind::Relay => "relay",
            Kind::Openrouter => "openrouter",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub name: String,
    pub kind: Kind,
    pub endpoint: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_ref: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Stored {
    version: String,
    selected: Option<String>,
    routes: Vec<Route>,
}

pub struct RouteTable {
    path: PathBuf,
    stored: Stored,
}

fn invalid(m: impl Into<String>) -> Error {
    Error::Invalid(m.into())
}

pub fn validate(r: &Route) -> Result<()> {
    if r.name.is_empty()
        || r.name.len() > 64
        || !r
            .name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.:".contains(&b))
    {
        return Err(invalid("route name must be 1-64 of [a-z0-9-_.:]"));
    }
    if r.model.trim().is_empty() || r.model.len() > 200 || r.model.contains(['\n', '\r', '"']) {
        return Err(invalid("route model is missing or invalid"));
    }
    match r.kind {
        Kind::Local | Kind::Relay => {
            let (_, port) = crate::qroc::parse_local_endpoint(&r.endpoint)?;
            if port == 8080 {
                return Err(invalid("port 8080 is forbidden"));
            }
            if r.key_ref.is_some() {
                return Err(invalid("a loopback route takes no key"));
            }
        }
        Kind::Openrouter => {
            if !(r.endpoint.starts_with("https://") || loopback_http(&r.endpoint))
                || r.endpoint.contains(['\n', '\r', '"', ' '])
            {
                return Err(invalid("OpenRouter endpoint must be an https URL"));
            }
            let key = r.key_ref.as_deref().ok_or_else(|| {
                invalid("OpenRouter route needs a key reference (an environment variable name)")
            })?;
            if key.is_empty()
                || key.len() > 64
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(invalid(
                    "key reference must be an environment variable name [A-Z0-9_]",
                ));
            }
        }
    }
    Ok(())
}

/// A route to OpenRouter's own endpoint (not a loopback stand-in) is a paid call: it is sent through
/// the shared API layer, never by the app itself.
pub fn via_layer(r: &Route) -> bool {
    r.kind == Kind::Openrouter && !loopback_http(&r.endpoint)
}

/// Whether the shared layer's key store holds an OpenRouter key (asked of the layer; no value is read
/// out). `STRATUM_API_CONFIG` moves the layer's settings, for tests only.
fn layer_has_key(env: &dyn Fn(&str) -> Option<String>) -> bool {
    let config = env("STRATUM_API_CONFIG")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config/stratum/api")));
    config.is_some_and(|c| stratum_api::keys::present(&c.join("keys.env"), LAYER_OPENROUTER_KEY))
}

/// Plain http is accepted only on loopback (a local stand-in for the OpenRouter endpoint), never on 8080.
fn loopback_http(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    crate::qroc::parse_local_endpoint(authority).is_ok_and(|(_, port)| port != 8080 && port != 0)
}

/// An endpoint a selected route may name: an OpenRouter URL (validated with its route), or a
/// loopback host:port that is not the chat model port.
pub(crate) fn guard_endpoint(endpoint: &str) -> Result<()> {
    if endpoint.starts_with("https://") || endpoint.starts_with("http://") {
        return Ok(());
    }
    let (_, port) = crate::qroc::parse_local_endpoint(endpoint)?;
    if port == 8080 || port == 0 {
        return Err(invalid("forbidden route port"));
    }
    Ok(())
}

/// Stable selected provider identity; a relay's ephemeral listener is transport only.
pub(crate) fn logical_identity(root: &Path) -> Result<Value> {
    let route = selected_at(root)?
        .or_else(|| seed_from_env(&env_var))
        .ok_or_else(|| invalid("No configured route for pipeline"))?;
    let mut identity = json!({"name":route.name,"kind":route.kind,"model":route.model});
    if route.kind != Kind::Relay {
        identity["endpoint"] = json!(route.endpoint);
    }
    Ok(identity)
}

/// Whether the route can send now, in words the interface can show (no variable names).
pub fn key_missing(r: &Route, env: &dyn Fn(&str) -> Option<String>) -> bool {
    if via_layer(r) {
        return !layer_has_key(env);
    }
    r.kind == Kind::Openrouter
        && r.key_ref
            .as_deref()
            .and_then(env)
            .is_none_or(|k| k.trim().is_empty())
}
pub const NEEDS_KEY: &str =
    "This OpenRouter route needs a key, and none is set in the shared key store";

pub(crate) fn env_var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// Seed route from launch variables, or None when they are not declared.
pub fn seed_from_env(env: &dyn Fn(&str) -> Option<String>) -> Option<Route> {
    let endpoint = env("QROC_GATEWAY")?;
    let model = env("QROC_MODEL")?;
    let provider = env("QROC_PROVIDER").unwrap_or_else(|| "local-gateway".into());
    // A relay label names a relay route ("codex:gpt-6.1-sol"); a bare model id goes to a local model directly.
    let kind = if ["codex:", "sheets:", "apikey:", "anthropic:", "llama:"]
        .iter()
        .any(|p| model.starts_with(p))
    {
        Kind::Relay
    } else {
        Kind::Local
    };
    let r = Route {
        name: provider,
        kind,
        endpoint,
        model,
        key_ref: None,
    };
    validate(&r).ok().map(|_| r)
}

/// The selected route of `<root>/routes.json`, read without seeding or writing anything.
/// None when there is no table or nothing is selected: the caller falls back to the launch variables.
pub fn selected_at(root: &Path) -> Result<Option<Route>> {
    let bytes = match fs::read(root.join("routes.json")) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let s: Stored = serde_json::from_slice(&bytes)?;
    if s.version != TABLE_VERSION {
        return Err(invalid("unknown routes table version"));
    }
    let Some(name) = s.selected else {
        return Ok(None);
    };
    let r = s
        .routes
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| invalid(format!("selected route '{name}' is not in the table")))?;
    validate(&r)?;
    Ok(Some(r))
}

impl RouteTable {
    /// Open `<root>/routes.json`; when absent, seed it from launch variables.
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with(root, &|k| std::env::var(k).ok())
    }
    pub fn open_with(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        crate::scope::validate_root(root)?;
        let path = root.join("routes.json");
        let stored = match fs::read(&path) {
            Ok(b) => {
                let s: Stored = serde_json::from_slice(&b)?;
                if s.version != TABLE_VERSION {
                    return Err(invalid("unknown routes table version"));
                }
                for r in &s.routes {
                    validate(r)?;
                }
                s
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut s = Stored {
                    version: TABLE_VERSION.into(),
                    ..Default::default()
                };
                if let Some(r) = seed_from_env(env) {
                    s.selected = Some(r.name.clone());
                    s.routes.push(r);
                }
                s
            }
            Err(e) => return Err(e.into()),
        };
        let t = Self { path, stored };
        if !t.path.exists() {
            t.save()?;
        }
        Ok(t)
    }
    fn save(&self) -> Result<()> {
        let pending = self.path.with_extension("json.pending");
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&pending)?;
        f.write_all(serde_json::to_string_pretty(&self.stored)?.as_bytes())?;
        f.sync_all()?;
        fs::rename(&pending, &self.path)?;
        Ok(())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn list(&self) -> &[Route] {
        &self.stored.routes
    }
    pub fn get(&self, name: &str) -> Option<&Route> {
        self.stored.routes.iter().find(|r| r.name == name)
    }
    pub fn selected(&self) -> Option<&Route> {
        self.stored.selected.as_deref().and_then(|n| self.get(n))
    }
    /// Add or replace a route by name.
    pub fn add(&mut self, r: Route) -> Result<()> {
        validate(&r)?;
        match self.stored.routes.iter_mut().find(|x| x.name == r.name) {
            Some(x) => *x = r,
            None => self.stored.routes.push(r),
        }
        self.save()
    }
    pub fn select(&mut self, name: &str) -> Result<()> {
        if self.get(name).is_none() {
            return Err(invalid(format!("no route named '{name}'")));
        }
        self.stored.selected = Some(name.into());
        self.save()
    }
    pub fn to_json(&self) -> Value {
        json!({"version":TABLE_VERSION,"selected":self.stored.selected,"routes":self.stored.routes.iter().map(|r|json!({"name":r.name,"kind":r.kind.as_str(),"endpoint":r.endpoint,"model":r.model,"key_ref":r.key_ref})).collect::<Vec<_>>()})
    }
}

/// A fully built OpenAI-compatible request. Only `masked()` may be shown.
pub struct ApiRequest {
    pub url: String,
    /// None when the request goes to the shared layer, which adds the key itself.
    key: Option<String>,
    pub body: Value,
}
impl ApiRequest {
    pub fn masked(&self) -> Value {
        let headers = if self.key.is_some() {
            json!({"Authorization":MASK,"Content-Type":"application/json"})
        } else {
            json!({"Content-Type":"application/json"})
        };
        json!({"method":"POST","url":self.url,"headers":headers,"body":self.body})
    }
    fn curl_config(&self, timeout_seconds: u64) -> String {
        let q = |s: &str| {
            format!(
                "\"{}\"",
                s.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
            )
        };
        let auth = self
            .key
            .as_ref()
            .map(|k| format!("header = {}\n", q(&format!("Authorization: Bearer {k}"))))
            .unwrap_or_default();
        format!(
            "url = {}\n{auth}header = \"Content-Type: application/json\"\ndata = {}\nmax-time = {}\n",
            q(&self.url),
            q(&self.body.to_string()),
            timeout_seconds
        )
    }
}

pub fn no_key_message(r: &Route) -> String {
    if via_layer(r) {
        return format!(
            "no key set: route '{}' needs an OpenRouter key in the shared key store",
            r.name
        );
    }
    format!(
        "no key set: route '{}' needs the environment variable {} (OpenRouter key)",
        r.name,
        r.key_ref.as_deref().unwrap_or("?")
    )
}

/// Build the OpenRouter request; the key comes from the environment variable the route names.
pub fn build_openrouter_request(
    r: &Route,
    messages: &Value,
    max_tokens: u32,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ApiRequest> {
    if r.kind != Kind::Openrouter {
        return Err(invalid("not an OpenRouter route"));
    }
    validate(r)?;
    if !messages.is_array() {
        return Err(invalid("messages must be an array"));
    }
    if via_layer(r) {
        // The layer reads the key from its store, prices the call and checks QROC's cap before it
        // is sent; the app sends no key.
        if !layer_has_key(env) {
            return Err(invalid(no_key_message(r)));
        }
        return Ok(ApiRequest {
            url: layer_chat(env),
            key: None,
            body: json!({"model":format!("openrouter:{}", r.model),"messages":messages,"max_tokens":max_tokens,"stream":false}),
        });
    }
    let key = r
        .key_ref
        .as_deref()
        .and_then(env)
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| invalid(no_key_message(r)))?;
    if key.contains(['\r', '\n', '"']) {
        return Err(invalid("key is malformed"));
    }
    Ok(ApiRequest {
        url: r.endpoint.clone(),
        key: Some(key),
        body: json!({"model":r.model,"messages":messages,"max_tokens":max_tokens,"stream":false}),
    })
}

pub struct Brief {
    pub content: String,
    pub receipt: Value,
    pub receipts_path: PathBuf,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The provider fields a receipt carries from a reply: generation id, served model, usage, billed charge
/// and currency, completion reason and retry identity. What the reply does not give is "unknown", never 0.
fn provider_fields(receipt: &mut Value, value: &Value) {
    let text = |v: &Value| v.as_str().map(|s| json!(s)).unwrap_or(json!("unknown"));
    receipt["generation_id"] = text(if value["generation_id"].is_string() {
        &value["generation_id"]
    } else {
        &value["id"]
    });
    receipt["served_model"] = text(&value["model"]);
    receipt["usage"] = value
        .get("usage")
        .filter(|u| u.as_object().is_some_and(|o| !o.is_empty()))
        .cloned()
        .unwrap_or(json!("unknown"));
    receipt["charge"] = if value["charge"]["amount"].is_string() {
        json!({"amount":value["charge"]["amount"],"currency":value["charge"]["currency"],"basis":value["charge"]["basis"]})
    } else if let Some(cost) = value["usage"]["cost"].as_f64() {
        // OpenRouter reports the billed cost of the generation in its credits (USD).
        json!({"amount":cost.to_string(),"currency":"USD","basis":"provider-reported usage.cost"})
    } else {
        json!({"amount":"unknown","currency":"unknown","basis":"reply carried no charge"})
    };
    receipt["completion_reason"] = text(if value["completion_reason"].is_string() {
        &value["completion_reason"]
    } else {
        &value["choices"][0]["finish_reason"]
    });
    receipt["retry_of"] = value.get("retry_of").cloned().unwrap_or(Value::Null);
    receipt["attempt"] = value.get("attempt").cloned().unwrap_or(json!("unknown"));
}

/// Receipt in the relay's schema, appended to `<root>/receipts.jsonl`.
fn write_receipt(root: &Path, receipt: &Value) -> Result<PathBuf> {
    let path = root.join("receipts.jsonl");
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{receipt}")?;
    f.sync_data()?;
    Ok(path)
}

/// Send one brief through a route and write its receipt. Never invents a reply.
pub fn send_brief(
    root: &Path,
    r: &Route,
    system: &str,
    prompt: &str,
    max_tokens: u32,
    timeout_seconds: u64,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Brief> {
    let start = Instant::now();
    let ts = now_ms();
    let id = format!("{ts}-{}", std::process::id());
    let messages = json!([{"role":"system","content":system},{"role":"user","content":prompt}]);
    let mut receipt = json!({"id":id,"ts":ts,"route":r.name,"kind":r.kind.as_str(),"requested_model":r.model,"reported_model":"unknown","binary_sha256":null,"max_tokens":max_tokens,"max_tokens_honoured":r.kind!=Kind::Relay,"finish_reason":null,"bytes_in":0,"bytes_out":0,"status":0,"ms":0,"child_exit":null,"budget_left":null});
    let result: Result<(Value, i32)> = match r.kind {
        Kind::Local | Kind::Relay => {
            let body =
                json!({"model":r.model,"messages":messages,"max_tokens":max_tokens,"stream":false});
            receipt["bytes_out"] = json!(body.to_string().len());
            crate::qroc::post_chat_completion(
                &r.endpoint,
                &body,
                timeout_seconds,
                &AtomicBool::new(false),
            )
            .map(|(v, _)| (v, 200))
        }
        Kind::Openrouter => match build_openrouter_request(r, &messages, max_tokens, env) {
            Err(e) => {
                receipt["status"] = json!("no-request");
                receipt["error"] = json!(e.to_string());
                Err(e)
            }
            Ok(req) => {
                receipt["bytes_out"] = json!(req.body.to_string().len());
                curl(&req, timeout_seconds).map(|(v, exit)| {
                    receipt["child_exit"] = json!(exit);
                    (v, 200)
                })
            }
        },
    };
    receipt["ms"] = json!(start.elapsed().as_millis());
    let out = match result {
        Err(e) => {
            if receipt["status"] == 0 {
                receipt["status"] = json!(502);
                receipt["error"] = json!(e.to_string());
            }
            write_receipt(root, &receipt)?;
            return Err(e);
        }
        Ok(v) => v,
    };
    let (value, status) = out;
    receipt["bytes_in"] = json!(value.to_string().len());
    provider_fields(&mut receipt, &value);
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .or_else(|| value["choices"][0]["text"].as_str())
        .map(str::to_owned);
    receipt["reported_model"] = value
        .get("model")
        .filter(|m| m.is_string())
        .cloned()
        .unwrap_or(json!("unknown"));
    receipt["finish_reason"] = json!(if value["choices"][0]["finish_reason"] == "length" {
        "length"
    } else {
        "stop"
    });
    let Some(content) = content.filter(|c| !c.trim().is_empty()) else {
        receipt["status"] = json!(502);
        receipt["error"] = json!("route returned no content");
        write_receipt(root, &receipt)?;
        return Err(invalid(format!("route '{}' returned no content", r.name)));
    };
    receipt["status"] = json!(status);
    receipt["content_sha256"] = json!(crate::qroc::digest(content.as_bytes()));
    let receipts_path = write_receipt(root, &receipt)?;
    Ok(Brief {
        content,
        receipt,
        receipts_path,
    })
}

/// One chat completion through the route generation uses for the store at `root`:
/// an OpenRouter selection goes through the request builder with a receipt in `<root>/receipts.jsonl`;
/// any other route is posted to its loopback endpoint as before.
pub(crate) fn complete_at(
    root: &Path,
    endpoint: &str,
    body: &Value,
    timeout_seconds: u64,
    cancel: &AtomicBool,
) -> Result<(Value, String)> {
    match selected_at(root)? {
        Some(r) if r.kind == Kind::Openrouter => {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(invalid("Generation cancelled"));
            }
            let max_tokens = body["max_tokens"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(4096);
            openrouter_completion(
                root,
                &r,
                &body["messages"],
                max_tokens,
                timeout_seconds,
                &env_var,
            )
        }
        _ => crate::qroc::post_chat_completion(endpoint, body, timeout_seconds, cancel),
    }
}

/// Send messages to an OpenRouter route and write its receipt; no key means no request.
pub(crate) fn openrouter_completion(
    root: &Path,
    r: &Route,
    messages: &Value,
    max_tokens: u32,
    timeout_seconds: u64,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(Value, String)> {
    let start = Instant::now();
    let ts = now_ms();
    let mut receipt = json!({"id":format!("{ts}-{}", std::process::id()),"ts":ts,"route":r.name,"kind":r.kind.as_str(),"requested_model":r.model,"reported_model":"unknown","binary_sha256":null,"max_tokens":max_tokens,"max_tokens_honoured":true,"finish_reason":null,"bytes_in":0,"bytes_out":0,"status":0,"ms":0,"child_exit":null,"budget_left":null});
    let result = build_openrouter_request(r, messages, max_tokens, env).and_then(|req| {
        receipt["bytes_out"] = json!(req.body.to_string().len());
        curl(&req, timeout_seconds)
    });
    receipt["ms"] = json!(start.elapsed().as_millis());
    let value = match result {
        Err(e) => {
            receipt["status"] = json!(if receipt["bytes_out"] == 0 {
                json!("no-request")
            } else {
                json!(502)
            });
            receipt["error"] = json!(e.to_string());
            write_receipt(root, &receipt)?;
            return Err(e);
        }
        Ok((v, exit)) => {
            receipt["child_exit"] = json!(exit);
            v
        }
    };
    receipt["bytes_in"] = json!(value.to_string().len());
    provider_fields(&mut receipt, &value);
    let model = value["model"].as_str().unwrap_or("").to_owned();
    if !model.is_empty() {
        receipt["reported_model"] = json!(model);
    }
    receipt["finish_reason"] = value["choices"][0]["finish_reason"].clone();
    match value["choices"][0]["message"]["content"]
        .as_str()
        .filter(|c| !c.trim().is_empty())
    {
        Some(c) => {
            receipt["status"] = json!(200);
            receipt["content_sha256"] = json!(crate::qroc::digest(c.as_bytes()));
        }
        None => {
            receipt["status"] = json!(502);
            receipt["error"] = json!("route returned no content");
        }
    }
    write_receipt(root, &receipt)?;
    Ok((value, model))
}

/// The key goes to curl on stdin, never on the command line.
fn curl(req: &ApiRequest, timeout_seconds: u64) -> Result<(Value, i32)> {
    let mut child = Command::new("/usr/bin/curl")
        .args(["-sS", "-K", "-", "--fail-with-body"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| invalid("curl stdin unavailable"))?
        .write_all(req.curl_config(timeout_seconds).as_bytes())?;
    let out = child.wait_with_output()?;
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        // The shared layer answers a refusal (cap, unknown pricing, no key) in plain words.
        let said = serde_json::from_slice::<Value>(&out.stdout)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned));
        return Err(invalid(match said {
            Some(why) => format!("OpenRouter request refused: {why}"),
            None => format!("OpenRouter request failed (curl exit {code})"),
        }));
    }
    Ok((serde_json::from_slice(&out.stdout)?, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "qroc-routes-{tag}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn or() -> Route {
        Route {
            name: "openrouter".into(),
            kind: Kind::Openrouter,
            endpoint: OPENROUTER_ENDPOINT.into(),
            model: "openai/gpt-oss-120b".into(),
            key_ref: Some("OPENROUTER_API_KEY".into()),
        }
    }
    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn env_seeds_absent_table_only() {
        let r = root("seed");
        let env = |k: &str| match k {
            "QROC_GATEWAY" => Some("127.0.0.1:8097".into()),
            "QROC_MODEL" => Some("codex:gpt-6.1-sol".into()),
            "QROC_PROVIDER" => Some("route-shim".into()),
            _ => None,
        };
        let t = RouteTable::open_with(&r, &env).unwrap();
        assert_eq!(t.list().len(), 1);
        assert_eq!(t.selected().unwrap().kind, Kind::Relay);
        let other = |k: &str| match k {
            "QROC_GATEWAY" => Some("127.0.0.1:8084".into()),
            "QROC_MODEL" => Some("m".into()),
            _ => None,
        };
        let t2 = RouteTable::open_with(&r, &other).unwrap();
        assert_eq!(
            t2.selected().unwrap().name,
            "route-shim",
            "existing table is not reseeded"
        );
    }
    #[test]
    fn receipt_carries_provider_fields_or_unknown() {
        let mut r = json!({});
        provider_fields(
            &mut r,
            &json!({"generation_id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":3},"charge":{"amount":"0.000012000","currency":"USD","basis":"usage x anthropic:claude-opus-5-5 price"},"completion_reason":"end_turn","retry_of":null,"attempt":1}),
        );
        assert_eq!(r["generation_id"], "msg_1");
        assert_eq!(r["charge"]["amount"], "0.000012000");
        assert_eq!(r["completion_reason"], "end_turn");
        let mut r = json!({});
        provider_fields(
            &mut r,
            &json!({"choices":[{"finish_reason":"stop"}],"usage":{}}),
        );
        assert_eq!(r["usage"], "unknown", "empty usage is unknown, never zero");
        assert_eq!(r["charge"]["amount"], "unknown");
        assert_eq!(r["generation_id"], "unknown");
        assert_eq!(r["completion_reason"], "stop");
        let env = |k: &str| match k {
            "QROC_GATEWAY" => Some("127.0.0.1:8097".into()),
            "QROC_MODEL" => Some("anthropic:claude-opus-5-5".into()),
            _ => None,
        };
        assert_eq!(seed_from_env(&env).unwrap().kind, Kind::Relay);
    }
    #[test]
    fn bare_model_seeds_local() {
        let env = |k: &str| match k {
            "QROC_GATEWAY" => Some("127.0.0.1:8084".into()),
            "QROC_MODEL" => Some("qwen".into()),
            _ => None,
        };
        assert_eq!(seed_from_env(&env).unwrap().kind, Kind::Local);
        assert!(seed_from_env(&none).is_none());
    }
    #[test]
    fn add_list_select_persist() {
        let r = root("ops");
        let mut t = RouteTable::open_with(&r, &none).unwrap();
        assert!(t.list().is_empty() && t.selected().is_none());
        t.add(or()).unwrap();
        t.add(Route {
            name: "local".into(),
            kind: Kind::Local,
            endpoint: "127.0.0.1:8084".into(),
            model: "qwen".into(),
            key_ref: None,
        })
        .unwrap();
        t.select("openrouter").unwrap();
        assert!(t.select("missing").is_err());
        let t = RouteTable::open_with(&r, &none).unwrap();
        assert_eq!(t.list().len(), 2);
        assert_eq!(t.selected().unwrap().name, "openrouter");
    }
    #[test]
    fn invalid_routes_refused() {
        let mut bad = or();
        bad.key_ref = None;
        assert!(validate(&bad).is_err());
        let mut bad = or();
        bad.key_ref = Some("sk-or-v1-abc".into());
        assert!(validate(&bad).is_err(), "a key value is not a reference");
        let mut bad = or();
        bad.endpoint = "http://openrouter.ai".into();
        assert!(validate(&bad).is_err());
        let l = Route {
            name: "l".into(),
            kind: Kind::Local,
            endpoint: "127.0.0.1:8080".into(),
            model: "m".into(),
            key_ref: None,
        };
        assert!(validate(&l).is_err());
        let l = Route {
            endpoint: "10.0.0.1:8084".into(),
            ..l
        };
        assert!(validate(&l).is_err());
    }
    #[test]
    fn openrouter_request_built_and_masked() {
        use std::os::unix::fs::PermissionsExt;
        // OpenRouter's own endpoint: the request goes to the shared API layer, which adds the key.
        let cfg = root("layer-config");
        fs::set_permissions(&cfg, fs::Permissions::from_mode(0o700)).unwrap();
        let keys = cfg.join("keys.env");
        fs::write(&keys, "OPENROUTER_API_KEY=SENTINEL-T402-LAYER-KEY\n").unwrap();
        fs::set_permissions(&keys, fs::Permissions::from_mode(0o600)).unwrap();
        let c = cfg.to_string_lossy().into_owned();
        let env = |k: &str| (k == "STRATUM_API_CONFIG").then(|| c.clone());
        assert!(!key_missing(&or(), &env));
        let req =
            build_openrouter_request(&or(), &json!([{"role":"user","content":"hi"}]), 256, &env)
                .unwrap();
        assert_eq!(req.url, LAYER_CHAT);
        assert_eq!(req.body["model"], "openrouter:openai/gpt-oss-120b");
        assert_eq!(req.body["max_tokens"], 256);
        assert_eq!(req.body["stream"], false);
        let config = req.curl_config(30);
        assert!(!config.contains("Authorization") && !config.contains("SENTINEL"));
        assert!(!req.masked().to_string().contains(MASK));
        // A loopback stand-in for the endpoint keeps the key it names, masked when shown.
        let stand = Route {
            endpoint: "http://127.0.0.1:18999/api/v1/chat/completions".into(),
            key_ref: Some("OPENROUTER_API_KEY".into()),
            ..or()
        };
        let env = |k: &str| (k == "OPENROUTER_API_KEY").then(|| "SENTINEL-T402-KEY".to_string());
        let req =
            build_openrouter_request(&stand, &json!([{"role":"user","content":"hi"}]), 256, &env)
                .unwrap();
        assert_eq!(req.url, stand.endpoint);
        assert_eq!(req.body["model"], "openai/gpt-oss-120b");
        let shown = req.masked().to_string();
        assert!(!shown.contains("SENTINEL") && shown.contains(MASK));
        assert!(
            req.curl_config(30)
                .contains("Authorization: Bearer SENTINEL-T402-KEY")
        );
    }
    #[test]
    fn openrouter_without_key_refuses_plainly_and_records() {
        let r = root("nokey");
        let e = send_brief(&r, &or(), "s", "p", 64, 5, &none)
            .err()
            .unwrap()
            .to_string();
        assert!(e.starts_with("no key set"), "{e}");
        let rows = fs::read_to_string(r.join("receipts.jsonl")).unwrap();
        assert!(rows.contains("\"no-request\"") && rows.contains("\"bytes_out\":0"));
        let empty = |_: &str| Some("  ".to_string());
        assert!(
            build_openrouter_request(&or(), &json!([]), 1, &empty)
                .err()
                .unwrap()
                .to_string()
                .starts_with("no key set")
        );
    }
}
