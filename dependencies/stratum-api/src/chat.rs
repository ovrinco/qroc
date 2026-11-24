//! The layer's transport (lifted from route-shim `paid.rs` and `codex::run`): one request to a
//! provider by curl, the key written to curl's standard input, never on a command line, the HTTP status
//! read so the guard can tell a refused request from an unknown charge. [`post`] streams the reply's
//! bytes to a callback as they arrive (for any wire format); [`complete`] speaks OpenAI chat
//! completions and Anthropic Messages, streamed, assembled, and classed for settlement.
use crate::{
    Billing, Error, Layer, Result, anthropic,
    keys::{self, Secret},
    money::not_billed,
    prices::read_json,
    stream::{self, Assembled},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub const OPENAI_BASE: &str = "https://api.openai.com";
pub const ANTHROPIC_BASE: &str = "https://api.anthropic.com";
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api";

/// The provider's own base address.
pub fn official_base(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some(OPENAI_BASE),
        "anthropic" => Some(ANTHROPIC_BASE),
        "openrouter" => Some(OPENROUTER_BASE),
        _ => None,
    }
}

/// A route of the layer's table (`routes.json`): a label names provider, model, base, output cap
/// and effort, as QROC's route labels do.
///
/// ```json
/// {"routes":{"openai:gpt-6.1-sol":{"provider":"openai","model":"gpt-6.1-sol","max_output_tokens":8000,"effort":"medium"}}}
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Route {
    pub label: String,
    pub provider: String,
    pub model: String,
    pub base: String,
    pub max_output: u64,
    pub effort: Option<String>,
}

impl Route {
    /// The route in the JSON form `anthropic::body` reads.
    pub fn as_value(&self) -> Value {
        let mut v = json!({"model":self.model});
        if let Some(e) = &self.effort {
            v["effort"] = json!(e);
        }
        v
    }
}

/// A base is a provider's own HTTPS address, or a loopback HTTP address (a local stand-in), never 8080.
pub fn check_base(provider: &str, base: &str) -> Result<()> {
    let b = base.trim_end_matches('/');
    if b.contains(['\n', '\r', '"', ' ']) || b.contains(":8080") {
        return Err(Error::Invalid(format!(
            "base address for {provider} is not allowed"
        )));
    }
    let official = match official_base(provider) {
        Some(b) => b,
        None => {
            return Err(Error::Invalid(format!(
                "provider '{provider}' is not one the layer speaks"
            )));
        }
    };
    if b == official {
        return Ok(());
    }
    let local = b
        .strip_prefix("http://")
        .and_then(|h| h.split('/').next())
        .and_then(|h| h.parse::<std::net::SocketAddr>().ok());
    match local {
        Some(a) if a.ip().is_loopback() && a.port() != 8080 => Ok(()),
        _ => Err(Error::Invalid(format!(
            "base address for {provider} is neither the provider's own nor loopback"
        ))),
    }
}

pub fn routes(path: &Path) -> Result<Vec<Route>> {
    let v = read_json(path)?;
    if v.to_string().contains(":8080") {
        return Err(Error::Invalid(
            "port 8080 is forbidden in the route table".into(),
        ));
    }
    let obj = v["routes"]
        .as_object()
        .ok_or_else(|| Error::Invalid("the route table has no routes".into()))?;
    let mut out = Vec::new();
    for (label, r) in obj {
        let provider = r["provider"]
            .as_str()
            .ok_or_else(|| Error::Invalid(format!("route {label} names no provider")))?
            .to_owned();
        let base = r["base"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| official_base(&provider).unwrap_or(OPENAI_BASE).to_owned());
        check_base(&provider, &base)?;
        // A wildcard route (`openrouter:*`) serves any model named after its prefix.
        let model = if label.ends_with(":*") {
            "*".to_owned()
        } else {
            r["model"]
                .as_str()
                .ok_or_else(|| Error::Invalid(format!("route {label} names no model")))?
                .to_owned()
        };
        let max_output = r["max_output_tokens"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| {
                Error::Invalid(format!("route {label} needs a positive max_output_tokens"))
            })?;
        let effort = r["effort"].as_str().map(str::to_owned);
        if effort
            .as_deref()
            .is_some_and(|e| !matches!(e, "minimal" | "low" | "medium" | "high" | "xhigh" | "max"))
        {
            return Err(Error::Invalid(format!(
                "route {label} has an invalid effort"
            )));
        }
        out.push(Route {
            label: label.clone(),
            provider,
            model,
            base: base.trim_end_matches('/').to_owned(),
            max_output,
            effort,
        });
    }
    Ok(out)
}

/// The route a label names: an exact entry, else the wildcard of its prefix (`openrouter:*` serves
/// `openrouter:<model>` with that model; whether the model is priced is the guard's to decide).
pub fn route(path: &Path, label: &str) -> Result<Route> {
    let all = routes(path)?;
    if let Some(r) = all.iter().find(|r| r.label == label) {
        return Ok(r.clone());
    }
    let wild = label.split_once(':').filter(|(_, m)| {
        !m.is_empty() && m.len() <= 200 && *m != "*" && !m.contains(['\n', '\r', '"', ' '])
    });
    if let Some((prefix, model)) = wild
        && let Some(w) = all.iter().find(|r| r.label == format!("{prefix}:*"))
    {
        return Ok(Route {
            label: label.to_owned(),
            model: model.to_owned(),
            ..w.clone()
        });
    }
    Err(Error::Invalid(format!("no route named {label}")))
}

fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

/// How one transport run ended.
#[derive(Debug)]
pub struct Ran {
    /// The reply body (whole or cut), up to 16 MiB (or the limit given to [`post_limited`]).
    pub body: Vec<u8>,
    /// The HTTP status, when curl finished and wrote it.
    pub status: Option<u16>,
    /// curl's exit code, when it exited.
    pub exit: Option<i32>,
    /// Why it ended early, empty when it finished: "spawn failed", "timeout", "cancelled", "nonzero exit";
    /// through the key service also "key service unreachable" and "key service refused" (nothing sent).
    pub reason: &'static str,
    /// Fixed classification generated from the numeric exit code, never subprocess stderr.
    pub curl_error: String,
}

impl Ran {
    /// Stable transport classification for the failure reason; no free-form subprocess text.
    pub fn said(&self) -> String {
        if self.curl_error.is_empty() {
            String::new()
        } else {
            format!("; curl said: {}", self.curl_error)
        }
    }
}

pub(crate) const BODY_LIMIT: usize = 16 * 1024 * 1024;
const STATUS_MARK: &[u8] = b"\n@@stratum-api-status:";

/// POST `body` to `url` with the given headers, the key's header written on curl's standard input
/// only. Each chunk of the reply body is handed to `on_chunk` as it arrives (return false to stop);
/// `cancelled` is polled (a client that went away) and the run ends at `timeout_ms`.
pub fn post(
    url: &str,
    headers: &[String],
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
    on_chunk: &mut dyn FnMut(&[u8]) -> bool,
) -> Ran {
    post_limited(
        url, headers, body, timeout_ms, cancelled, on_chunk, BODY_LIMIT,
    )
}

/// [`post`] keeping up to `limit` bytes of the reply body (an image reply carries its images whole).
pub fn post_limited(
    url: &str,
    headers: &[String],
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
    on_chunk: &mut dyn FnMut(&[u8]) -> bool,
    limit: usize,
) -> Ran {
    post_transport(
        url, headers, body, timeout_ms, cancelled, on_chunk, limit, false,
    )
}

/// The broker's transport must ignore caller-controlled curl configuration and proxy environment.
pub(crate) fn post_service_limited(
    url: &str,
    headers: &[String],
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
    on_chunk: &mut dyn FnMut(&[u8]) -> bool,
    limit: usize,
) -> Ran {
    post_transport(
        url, headers, body, timeout_ms, cancelled, on_chunk, limit, true,
    )
}

fn curl_command(isolated: bool) -> Command {
    let mut command = Command::new("/usr/bin/curl");
    if isolated {
        command
            .args(["-q", "--proxy", "", "--proto", "=https,http"])
            .env_clear();
    }
    // Never capture free-form curl errors: curl itself may already have split/encoded a credential.
    // Diagnostics are generated only from numeric exit status, not from stderr or request bytes.
    command.stderr(Stdio::null());
    command.args(["-sS", "-K", "-"]);
    command
}

fn header_credentials(headers: &[String]) -> Vec<zeroize::Zeroizing<String>> {
    let mut values = Vec::new();
    for header in headers {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let name = name.to_ascii_lowercase();
        let value = value.trim();
        if (name.contains("authorization") || name.contains("key") || name.contains("token"))
            && !value.is_empty()
        {
            values.push(zeroize::Zeroizing::new(value.to_owned()));
            if name.contains("authorization")
                && let Some((_, credential)) = value.split_once(' ')
                && !credential.is_empty()
            {
                values.push(zeroize::Zeroizing::new(credential.to_owned()));
            }
        }
    }
    values
}
fn forward(
    filter: &mut Option<keys::Redactor>,
    callback: &mut dyn FnMut(&[u8]) -> bool,
    raw: &[u8],
) -> bool {
    match filter {
        Some(filter) => {
            let clean = filter.push(raw, false);
            clean.is_empty() || callback(&clean)
        }
        None => callback(raw),
    }
}

#[allow(clippy::too_many_arguments)]
fn post_transport(
    url: &str,
    headers: &[String],
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
    on_chunk: &mut dyn FnMut(&[u8]) -> bool,
    limit: usize,
    isolated: bool,
) -> Ran {
    let credentials = header_credentials(headers);
    // The service must apply its complete multi-provider set in one pass, never a shorter subset first.
    // The legacy reply hides only this request's own credentials: the environment never rewrites it.
    let mut filter = if isolated {
        None
    } else {
        Some(keys::Redactor::new(credentials.clone()))
    };
    let mut input = format!("url = {}\n", quote(url));
    for h in headers
        .iter()
        .map(String::as_str)
        .chain(["Content-Type: application/json"])
    {
        input.push_str(&format!("header = {}\n", quote(h)));
    }
    input.push_str(&format!(
        "data = {}\nmax-time = {}\nkeepalive-time = 20\nno-buffer\nwrite-out = {}\n",
        quote(&body.to_string()),
        (timeout_ms as f64 / 1000.0).max(0.001),
        quote(&format!(
            "{}%{{http_code}}",
            String::from_utf8_lossy(STATUS_MARK).replace('\n', "\\n")
        ))
    ));
    let ended = |reason, exit| Ran {
        body: Vec::new(),
        status: None,
        exit,
        reason,
        curl_error: String::new(),
    };
    let mut child = match curl_command(isolated)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return ended("spawn failed", None),
    };
    let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return ended("spawn failed", None);
    };
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let reader = thread::spawn(move || {
        let mut b = [0; 16384];
        loop {
            match stdout.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(b[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let start = Instant::now();
    let mut last_check = start;
    let mut all: Vec<u8> = Vec::new();
    let mut forwarded = 0usize;
    let mut reason = "";
    let mut stopped = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => {
                if all.len() + chunk.len() <= limit + STATUS_MARK.len() + 8 {
                    all.extend_from_slice(&chunk);
                }
                // Hold back what may be the start of the status mark curl writes at the end.
                let from = forwarded.saturating_sub(STATUS_MARK.len());
                let end = find(&all[from..], STATUS_MARK)
                    .map(|i| from + i)
                    .unwrap_or_else(|| all.len().saturating_sub(STATUS_MARK.len() + 4));
                if end > forwarded && !stopped {
                    if !forward(&mut filter, on_chunk, &all[forwarded..end]) {
                        stopped = true;
                    }
                    forwarded = end;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // A caller's check may itself block (a peek with a read timeout): ask at most every 200 ms,
        // never once per chunk, so a long stream of small chunks is not slowed by it.
        if stopped
            || (last_check.elapsed() >= Duration::from_millis(200) && {
                last_check = Instant::now();
                cancelled()
            })
        {
            reason = "cancelled";
            break;
        }
        if start.elapsed() > Duration::from_millis(timeout_ms + 2000) {
            reason = "timeout";
            break;
        }
    }
    if !reason.is_empty() {
        let _ = child.kill();
    }
    let status = child.wait().ok();
    let _ = writer.join();
    let _ = reader.join();
    while let Ok(chunk) = rx.try_recv() {
        if all.len() + chunk.len() <= limit + STATUS_MARK.len() + 8 {
            all.extend_from_slice(&chunk);
        }
    }
    let exit = status.and_then(|s| s.code());
    let (body_end, http) = match find(&all, STATUS_MARK) {
        Some(i) => {
            let code = String::from_utf8_lossy(&all[i + STATUS_MARK.len()..])
                .trim()
                .parse::<u16>()
                .ok();
            (i, code.filter(|c| *c != 0))
        }
        None => (all.len(), None),
    };
    if reason.is_empty() && body_end > forwarded {
        forward(&mut filter, on_chunk, &all[forwarded..body_end]);
    }
    if let Some(filter) = filter.as_mut() {
        let tail = filter.push(&[], true);
        if !stopped && !tail.is_empty() {
            on_chunk(&tail);
        }
    }
    if reason.is_empty() && exit != Some(0) {
        reason = "nonzero exit";
    }
    all.truncate(body_end);
    let curl_error = transport_failure(exit);
    Ran {
        // Service response bytes leave only through its filtered callback, never via a Debug/log object.
        body: if isolated {
            Vec::new()
        } else {
            keys::redact_credentials(&all, credentials)
        },
        status: if exit == Some(0) { http } else { None },
        exit,
        reason,
        curl_error,
    }
}

fn transport_failure(exit: Option<i32>) -> String {
    match exit {
        Some(code) if code != 0 => format!("transport failure (curl exit {code})"),
        _ => String::new(),
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// The headers that carry `provider`'s key, and the provider's own version header.
pub fn auth_headers(provider: &str, key: &Secret) -> Vec<String> {
    if provider == "anthropic" {
        vec![
            format!("x-api-key: {}", key.expose()),
            anthropic::VERSION_HEADER.into(),
        ]
    } else {
        vec![format!("Authorization: Bearer {}", key.expose())]
    }
}

/// Keep a status-only failure record for reconciliation. Provider payloads are deliberately omitted;
/// transforming or retaining an already-partial credential cannot be made safe by later matching.
pub fn keep(dir: &Path, id: &str, status: u16, body: &[u8], key: Option<&Secret>) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(dir.join(format!("{id}.provider-{status}.txt")))
    {
        // Failed provider payloads are not diagnostics. Do not retain, trim or re-encode them.
        let _ = (body, key);
        let _ = f.write_all(
            format!("provider HTTP {status}; response body omitted for credential isolation\n")
                .as_bytes(),
        );
    }
}

/// One chat request to send: OpenAI-form messages to a provider's chat endpoint.
pub struct Call<'a> {
    pub id: &'a str,
    pub provider: &'a str,
    pub base: &'a str,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub messages: &'a Value,
    pub max_output: u64,
    pub timeout_ms: u64,
    /// Where failed or refused provider bodies are kept (masked); the layer's own folder when None.
    pub keep_dir: Option<&'a Path>,
}

/// How a chat request ended: the reply in OpenAI chat form (id, model, choices, usage and
/// `completion_reason`), or the failure's HTTP status for the caller; and its billing class.
pub struct Outcome {
    pub reply: std::result::Result<Value, u16>,
    pub billing: Billing,
}

/// The request body for `call` in its provider's own form, streamed with its final usage.
pub fn request_body(call: &Call) -> Value {
    if call.provider == "anthropic" {
        let mut r = json!({"model":call.model});
        if let Some(e) = call.effort {
            r["effort"] = json!(e);
        }
        anthropic::body(&r, call.messages, call.max_output)
    } else if call.provider == "openrouter" {
        // OpenRouter: OpenAI's chat form, its output cap as `max_tokens`, usage asked for in the stream.
        let mut b = json!({"model":call.model,"messages":call.messages,"max_tokens":call.max_output,"stream":true,"stream_options":{"include_usage":true},"usage":{"include":true}});
        if let Some(e) = call.effort {
            b["reasoning"] = json!({"effort":e});
        }
        b
    } else {
        let mut b = json!({"model":call.model,"messages":call.messages,"max_completion_tokens":call.max_output,"stream":true,"stream_options":{"include_usage":true}});
        if let Some(e) = call.effort {
            b["reasoning_effort"] = json!(e);
        }
        b
    }
}

/// Send one admitted chat request with the key from the layer's store, and class how it ended.
pub fn complete(layer: &Layer, call: &Call, cancelled: &dyn Fn() -> bool) -> Outcome {
    let keep_dir = call
        .keep_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| layer.home.join("failed"));
    let not_sent = |why: String| Outcome {
        reply: Err(502),
        billing: Billing::NotSent(why),
    };
    if let Err(e) = check_base(call.provider, call.base) {
        return not_sent(e.to_string());
    }
    let Some(var) = keys::var_for(call.provider) else {
        return not_sent(format!("provider '{}' has no key", call.provider));
    };
    let key = match layer.key(var) {
        Ok(k) => k,
        Err(_) => return not_sent("key not in the key store".into()),
    };
    let p = call.provider;
    let base = call.base.trim_end_matches('/');
    let url = if p == "anthropic" {
        format!("{base}/v1/messages")
    } else {
        format!("{base}/v1/chat/completions")
    };
    let ran = post_key(
        &key,
        p,
        &url,
        &request_body(call),
        call.timeout_ms,
        cancelled,
        &mut |_| true,
        BODY_LIMIT,
    );
    let text = String::from_utf8_lossy(&ran.body).into_owned();
    let assembled = |t: &str| {
        if p == "anthropic" {
            stream::anthropic(t)
        } else {
            stream::openai(t)
        }
    };
    if !ran.reason.is_empty() {
        // A stream the far side cut: its charge is exact only if its final usage had arrived.
        let a = assembled(&text);
        if !text.is_empty() {
            keep(
                &keep_dir,
                call.id,
                ran.status.unwrap_or(502),
                text.as_bytes(),
                Some(&key),
            );
        }
        let billing = match (ran.reason, ran.exit) {
            ("spawn failed", _) => Billing::NotSent("curl did not start".into()),
            ("key service unreachable", _) => {
                Billing::NotSent("the key service could not be reached".into())
            }
            ("key service refused", _) => {
                Billing::NotSent("the key service refused the request; nothing was sent".into())
            }
            (_, Some(n @ (6 | 7))) => {
                Billing::NotSent(format!("curl could not connect (exit {n})"))
            }
            _ if a.usage_final => usage(a.reply.clone()),
            (_, Some(28)) | ("timeout", _) => {
                Billing::Unknown(format!("provider timed out (curl exit 28){}", ran.said()))
            }
            (reason, Some(n)) if !text.is_empty() => Billing::Unknown(format!(
                "stream cut before its final usage ({reason}, curl exit {n}){}",
                ran.said()
            )),
            (reason, exit) => {
                Billing::Unknown(format!("{reason} (curl exit {exit:?}){}", ran.said()))
            }
        };
        if ran.reason == "nonzero exit" && a.whole() {
            // Cut after the reply was whole (its stop reason in): deliver it; the charge as above.
            return Outcome {
                reply: deliver(p, a.reply),
                billing,
            };
        }
        let status = if ran.reason == "timeout" { 504 } else { 502 };
        return Outcome {
            reply: Err(status),
            billing,
        };
    }
    let Some(status) = ran.status else {
        return Outcome {
            reply: Err(502),
            billing: Billing::Unknown("no HTTP status".into()),
        };
    };
    if status != 200 {
        keep(&keep_dir, call.id, status, text.as_bytes(), Some(&key));
        let billing = if not_billed(status) {
            Billing::Rejected(status)
        } else {
            Billing::Unknown(format!("provider HTTP {status}"))
        };
        return Outcome {
            reply: Err(502),
            billing,
        };
    }
    let a = if stream::is_stream(&text) {
        assembled(&text)
    } else {
        // A provider that ignored `stream` and answered whole.
        let Ok(raw) = serde_json::from_str::<Value>(&text) else {
            keep(&keep_dir, call.id, status, text.as_bytes(), Some(&key));
            return Outcome {
                reply: Err(502),
                billing: Billing::Unknown("reply is not JSON".into()),
            };
        };
        let stopped =
            !raw["stop_reason"].is_null() || !raw["choices"][0]["finish_reason"].is_null();
        Assembled {
            usage_final: raw["usage"].is_object(),
            stopped,
            error: None,
            reply: raw,
        }
    };
    let billing = match (&a.error, a.usage_final) {
        (_, true) => usage(a.reply.clone()),
        (Some(e), false) => Billing::Unknown(
            String::from_utf8_lossy(&keys::mask(e.as_bytes(), &[&key])).into_owned(),
        ),
        (None, false) if stream::is_stream(&text) => {
            Billing::Unknown("stream ended without its final usage".into())
        }
        (None, false) => Billing::Unknown("reply carried no usage".into()),
    };
    if !a.whole() {
        keep(&keep_dir, call.id, status, text.as_bytes(), Some(&key));
        return Outcome {
            reply: Err(502),
            billing,
        };
    }
    Outcome {
        reply: deliver(p, a.reply),
        billing,
    }
}

/// The billing class of a reply that carried its usage.
pub fn usage(reply: Value) -> Billing {
    let served = reply["model"].as_str().unwrap_or("unknown").to_owned();
    Billing::Usage { reply, served }
}

/// The reply in OpenAI chat form, keeping id, model, usage and the completion reason.
fn deliver(p: &str, raw: Value) -> std::result::Result<Value, u16> {
    let value = if p == "anthropic" {
        anthropic::normalise(&raw)
    } else {
        let mut v = raw.clone();
        v["completion_reason"] = raw["choices"][0]["finish_reason"].clone();
        Some(v)
    };
    value.ok_or(502)
}

/// Dispatches through the broker when switched; the layer never obtains its key.
#[allow(clippy::too_many_arguments)]
pub fn post_key(
    key: &Secret,
    provider: &str,
    url: &str,
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
    on_chunk: &mut dyn FnMut(&[u8]) -> bool,
    limit: usize,
) -> Ran {
    if let Some((socket, var)) = key.broker() {
        return crate::keysvc::call(
            socket, var, url, body, timeout_ms, cancelled, on_chunk, limit,
        );
    }
    post_limited(
        url,
        &auth_headers(provider, key),
        body,
        timeout_ms,
        cancelled,
        on_chunk,
        limit,
    )
}

#[cfg(test)]
mod transport_policy_tests {
    use super::*;
    #[test]
    fn stderr_is_discarded_at_spawn_for_both_transports_and_only_exit_code_is_reported() {
        for isolated in [false, true] {
            let mut messages = Vec::new();
            for key in [
                "sk-fixture-PREFIX!TAIL-SENTINEL",
                "sk-other-PREFIX?OTHER-SENTINEL",
            ] {
                // Curl itself echoes this invalid option. There must be no stderr pipe to capture it.
                let mut child = curl_command(isolated)
                    .arg(format!("--{key}"))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                assert!(child.stderr.is_none());
                let exit = child.wait().unwrap();
                assert!(!exit.success());
                let classification = transport_failure(exit.code());
                assert!(!classification.contains("SENTINEL"));
                messages.push(classification);
            }
            assert_eq!(
                messages[0], messages[1],
                "diagnostics depend on exit code, not request bytes"
            );
        }
    }
    #[test]
    fn failure_log_is_status_only_even_when_a_legacy_tail_would_split_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let key = Secret::local("sk-fixture-PREFIX!TAIL-SENTINEL".into());
        let mut body = vec![b'x'; 20000];
        body.extend_from_slice(key.expose().as_bytes());
        body.extend(vec![b'y'; 64 * 1024 - key.expose().len() + 5]);
        // The old raw tail began five bytes inside the credential and leaked the remainder.
        keep(dir.path(), "fixture", 502, &body, Some(&key));
        let logged = fs::read(dir.path().join("fixture.provider-502.txt")).unwrap();
        assert!(logged.len() <= 64 * 1024);
        assert!(!String::from_utf8_lossy(&logged).contains("TAIL-SENTINEL"));
        assert!(!String::from_utf8_lossy(&logged).contains("PREFIX"));
        assert_eq!(
            logged,
            b"provider HTTP 502; response body omitted for credential isolation\n"
        );
        keep(
            dir.path(),
            "fixture",
            502,
            b"entirely different provider text",
            Some(&key),
        );
        assert_eq!(
            fs::read(dir.path().join("fixture.provider-502.txt")).unwrap(),
            logged
        );
    }
    #[test]
    fn failure_log_ignores_partial_transformed_and_json_escaped_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let key = Secret::local("sk-fixture-PREFIX/TAIL-SENTINEL".into());
        for body in [
            b"***!TAIL-SENTINEL".as_slice(),
            b"TAIL-SENTINEL",
            b"sk-fixture-PREFIX\\/TAIL-SENTINEL",
        ] {
            keep(dir.path(), "fixture", 401, body, Some(&key));
            assert_eq!(
                fs::read(dir.path().join("fixture.provider-401.txt")).unwrap(),
                b"provider HTTP 401; response body omitted for credential isolation\n"
            );
        }
    }
    #[test]
    fn legacy_transport_keeps_its_original_curl_options_and_only_broker_is_isolated() {
        let legacy = curl_command(false);
        assert_eq!(legacy.get_args().collect::<Vec<_>>(), ["-sS", "-K", "-"]);
        let broker = curl_command(true);
        assert_eq!(
            broker.get_args().collect::<Vec<_>>(),
            [
                "-q",
                "--proxy",
                "",
                "--proto",
                "=https,http",
                "-sS",
                "-K",
                "-"
            ]
        );
    }
}
