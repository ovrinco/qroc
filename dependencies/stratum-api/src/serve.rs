//! The layer's local service: a loopback HTTP listener (socket-activated by the user unit, or bound
//! itself), one thread per connection, refusing any peer that is not loopback, exiting after an idle
//! spell with no connection open. A tool names itself in the path:
//! `POST /tool/<tool>/v1/chat/completions` (OpenAI-form chat; the route label in `model` picks
//! provider and model), `POST /tool/<tool>/v1/responses` (the Responses path) and
//! `POST /tool/<tool>/v1/images/generations` and `.../v1/images/edits` (the image route).
use crate::{
    Ask, Error, Layer, Result,
    chat::{self, Call},
    money,
    prices::fmt,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8098";
pub const IDLE: Duration = Duration::from_secs(600);
const LIMIT: usize = 16 * 1024 * 1024;
const MAX_OPEN: usize = 16;

/// The listening socket: the one systemd passed (socket activation), or one bound here. Only a
/// loopback address, never port 8080.
pub fn listener(addr: &str) -> Result<TcpListener> {
    if let Some(l) = activated()? {
        check_listen(
            &l.local_addr()
                .map_err(|e| Error::Invalid(format!("activated socket: {e}")))?,
        )?;
        return Ok(l);
    }
    let a: SocketAddr = addr
        .parse()
        .map_err(|_| Error::Invalid(format!("'{addr}' is not an address")))?;
    check_listen(&a)?;
    TcpListener::bind(a).map_err(|e| Error::Invalid(format!("cannot listen on {a}: {e}")))
}

fn check_listen(a: &SocketAddr) -> Result<()> {
    if !a.ip().is_loopback() || a.port() == 8080 {
        return Err(Error::Invalid(
            "the service listens on loopback only, never on port 8080".into(),
        ));
    }
    Ok(())
}

/// The socket systemd handed over (`LISTEN_PID` is this process, `LISTEN_FDS` at least 1: fd 3).
#[allow(unsafe_code)]
fn activated() -> Result<Option<TcpListener>> {
    use std::os::fd::FromRawFd;
    let pid_ok = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
        == Some(std::process::id());
    let fds = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|n| n.parse::<i32>().ok())
        .unwrap_or(0);
    if !pid_ok || fds < 1 {
        return Ok(None);
    }
    // SAFETY: systemd's socket-activation protocol: when LISTEN_PID names this process and LISTEN_FDS
    // is at least 1, descriptor 3 is an open listening socket handed to this process alone, and
    // nothing else in this process has taken ownership of it (it is read once, at start).
    let l = unsafe { TcpListener::from_raw_fd(3) };
    Ok(Some(l))
}

/// Serve until no connection has been open for `idle`.
pub fn run(layer: Layer, l: TcpListener, idle: Duration, timeout_ms: u64) -> Result<()> {
    l.set_nonblocking(true)
        .map_err(|e| Error::Invalid(format!("listener: {e}")))?;
    let open = Arc::new(AtomicUsize::new(0));
    let layer = Arc::new(layer);
    let mut last = Instant::now();
    loop {
        match l.accept() {
            Ok((s, peer)) => {
                last = Instant::now();
                if !peer.ip().is_loopback() {
                    drop(s);
                    continue;
                }
                if open.load(Ordering::SeqCst) >= MAX_OPEN {
                    let mut s = s;
                    respond(
                        &mut s,
                        503,
                        &json!({"error":"the service is busy; try again"}),
                    );
                    continue;
                }
                open.fetch_add(1, Ordering::SeqCst);
                let (open, layer) = (Arc::clone(&open), Arc::clone(&layer));
                thread::spawn(move || {
                    let _ = s.set_nonblocking(false);
                    connection(&layer, s, timeout_ms);
                    open.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if open.load(Ordering::SeqCst) > 0 {
                    last = Instant::now();
                } else if last.elapsed() >= idle {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// One parsed request.
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Read one request with a `Content-Length` body (chunked bodies are refused).
pub fn read_request(s: &mut TcpStream) -> std::result::Result<Request, u16> {
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut bytes = Vec::new();
    let mut b = [0; 16384];
    let head_end = loop {
        let n = s.read(&mut b).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&b[..n]);
        if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if bytes.len() > 64 * 1024 {
            return Err(431);
        }
    };
    let head = std::str::from_utf8(&bytes[..head_end]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (
        first.next().unwrap_or("").to_owned(),
        first.next().unwrap_or("").to_owned(),
    );
    let mut headers = Vec::new();
    let mut length = 0usize;
    for l in lines {
        let (k, v) = l.split_once(':').ok_or(400u16)?;
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_owned());
        if k == "transfer-encoding" {
            return Err(411);
        }
        if k == "content-length" {
            length = v.parse().map_err(|_| 400u16)?;
        }
        if k == "expect" && v.eq_ignore_ascii_case("100-continue") {
            let _ = s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
        }
        headers.push((k, v));
    }
    // A reference-image edit carries its images in the body.
    let limit = if path.ends_with("/v1/images/edits") {
        crate::images::REQUEST_LIMIT
    } else {
        LIMIT
    };
    if length > limit {
        return Err(413);
    }
    let mut body = bytes[head_end + 4..].to_vec();
    while body.len() < length {
        let n = s.read(&mut b).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&b[..n]);
    }
    body.truncate(length);
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Bad Gateway",
    }
}

/// Write a whole JSON reply and close.
pub fn respond(s: &mut TcpStream, status: u16, v: &Value) {
    let b = v.to_string();
    let h = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        b.len()
    );
    let _ = s
        .write_all(h.as_bytes())
        .and_then(|_| s.write_all(b.as_bytes()));
}

/// Start a streamed reply (server-sent events, ended by closing the connection).
pub fn stream_start(s: &mut TcpStream) -> std::io::Result<()> {
    s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")
}

/// True when the client closed its side.
pub fn disconnected(client: &TcpStream) -> bool {
    let mut b = [0; 1];
    let _ = client.set_nonblocking(true);
    let gone = matches!(client.peek(&mut b), Ok(0));
    let _ = client.set_nonblocking(false);
    gone
}

/// The tool named in a service path `/tool/<tool>/<rest>`.
pub fn split_tool(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/tool/")?;
    let (tool, rest) = rest.split_once('/')?;
    crate::caps::valid_tool(tool).then_some((tool, rest))
}

fn connection(layer: &Layer, mut s: TcpStream, timeout_ms: u64) {
    let req = match read_request(&mut s) {
        Ok(r) => r,
        Err(status) => {
            return respond(
                &mut s,
                status,
                &json!({"error":"the request could not be read"}),
            );
        }
    };
    let Some((tool, rest)) = split_tool(&req.path) else {
        return respond(
            &mut s,
            404,
            &json!({"error":"no such path; a tool calls /tool/<tool>/v1/..."}),
        );
    };
    match (req.method.as_str(), rest) {
        ("POST", "v1/chat/completions") => {
            chat_completions(layer, tool, &req.body, &mut s, timeout_ms)
        }
        ("POST", "v1/responses") => {
            crate::responses::handle(layer, tool, &req.body, &mut s, timeout_ms)
        }
        ("POST", "v1/images/generations") => crate::images::handle(
            layer,
            tool,
            crate::images::Kind::Generate,
            &req.body,
            &mut s,
            timeout_ms,
        ),
        ("POST", "v1/images/edits") => crate::images::handle(
            layer,
            tool,
            crate::images::Kind::Edit,
            &req.body,
            &mut s,
            timeout_ms,
        ),
        _ => respond(&mut s, 404, &json!({"error":"no such path for this tool"})),
    }
}

/// A provider's name as people know it.
pub fn provider_name(provider: &str) -> &str {
    match provider {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "openrouter" => "OpenRouter",
        other => other,
    }
}

/// OpenAI-form chat for `tool`: the route label in `model` picks provider and model; admitted under
/// the tool's cap before it is sent, settled after, replied whole (no streaming on this path).
pub fn chat_completions(
    layer: &Layer,
    tool: &str,
    body: &[u8],
    s: &mut TcpStream,
    timeout_ms: u64,
) {
    let Ok(body) = serde_json::from_slice::<Value>(body) else {
        return respond(s, 400, &json!({"error":"the body is not JSON"}));
    };
    if body["stream"] == true {
        return respond(
            s,
            400,
            &json!({"error":"streamed replies are not offered on this path"}),
        );
    }
    if !body["messages"].is_array() || body.get("max_tokens").is_some_and(|v| v.as_u64().is_none())
    {
        return respond(s, 400, &json!({"error":"invalid request"}));
    }
    let label = body["model"].as_str().unwrap_or("");
    let route = match layer.route(label) {
        Ok(r) => r,
        Err(e) => return respond(s, 400, &json!({"error":e.to_string()})),
    };
    // A provider whose key is not in the store is refused before anything is reserved or sent.
    if let Some(var) = crate::keys::var_for(&route.provider)
        && !layer.has_key(var)
    {
        return respond(
            s,
            503,
            &json!({"error":format!("This route needs an {} key in the shared key store, and none is there", provider_name(&route.provider))}),
        );
    }
    let max_out = body["max_tokens"]
        .as_u64()
        .map_or(route.max_output, |n| n.min(route.max_output));
    let fp = money::fingerprint(label, &body["messages"], max_out);
    let ask = Ask {
        tool,
        id: None,
        route: label,
        provider: &route.provider,
        model: &route.model,
        input_bound: money::input_bound(&body["messages"]),
        max_output: max_out,
        fingerprint: &fp,
        inner: None,
    };
    let admitted = match layer.admit(&ask) {
        Ok(a) => a,
        Err(r) => return respond(s, r.status(), &json!({"error":r.reason,"detail":r.figures})),
    };
    let call = Call {
        id: &admitted.id,
        provider: &route.provider,
        base: &route.base,
        model: &route.model,
        effort: route.effort.as_deref(),
        messages: &body["messages"],
        max_output: max_out,
        timeout_ms,
        keep_dir: None,
    };
    let client = s.try_clone().ok();
    let out = chat::complete(layer, &call, &|| client.as_ref().is_some_and(disconnected));
    let settled = layer.settle(&admitted, out.billing);
    let charge = json!({"amount":settled.charge.map(fmt).unwrap_or_else(|| "unknown".into()),"currency":admitted.currency,"basis":settled.basis});
    match out.reply {
        Ok(v) => {
            let content = v["choices"][0]["message"]["content"].clone();
            let finish = if v["choices"][0]["finish_reason"] == "length" {
                "length"
            } else {
                "stop"
            };
            respond(
                s,
                200,
                &json!({"id":admitted.id,"choices":[{"index":0,"message":{"role":"assistant","content":content},"finish_reason":finish}],"model":v["model"],"route":label,"usage":v["usage"],"generation_id":v["id"],"completion_reason":v["completion_reason"],"charge":charge,"attempt":admitted.attempt,"retry_of":admitted.retry_of}),
            );
        }
        Err(status) => respond(
            s,
            status,
            &json!({"error":"the provider call failed","id":admitted.id,"charge":charge}),
        ),
    }
}
