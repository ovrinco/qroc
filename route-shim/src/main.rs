mod anthropic;
mod apikey;
mod codex;
mod money;
mod paid;
mod routes;
mod stream;
use codex::{Failure, ResultData};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static IDS: AtomicU64 = AtomicU64::new(0);
const LIMIT: usize = 4 * 1024 * 1024;
fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
fn read_http(
    s: &mut TcpStream,
    client: Option<&TcpStream>,
    timeout: u64,
) -> Result<(u16, Vec<u8>), Failure> {
    let start = Instant::now();
    let mut bytes = Vec::new();
    let mut boundary = None;
    let mut length = None;
    let mut status = 200;
    loop {
        if start.elapsed() > Duration::from_millis(timeout) {
            return Err(Failure {
                status: 504,
                exit: None,
            });
        }
        if client.is_some_and(codex::disconnected) {
            return Err(Failure {
                status: 502,
                exit: None,
            });
        }
        let mut b = [0; 8192];
        match s.read(&mut b) {
            Ok(0) => {
                return Err(Failure {
                    status: 502,
                    exit: None,
                });
            }
            Ok(n) => bytes.extend_from_slice(&b[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => {
                return Err(Failure {
                    status: 502,
                    exit: None,
                });
            }
        }
        if bytes.len() > LIMIT {
            return Err(Failure {
                status: 502,
                exit: None,
            });
        }
        if boundary.is_none()
            && let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n")
        {
            let header = std::str::from_utf8(&bytes[..p]).map_err(|_| Failure {
                status: 400,
                exit: None,
            })?;
            let mut lines = header.split("\r\n");
            let first = lines.next().unwrap_or("");
            if first.starts_with("HTTP/") {
                status = first
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .ok_or(Failure {
                        status: 502,
                        exit: None,
                    })?;
            } else if first != "POST /v1/chat/completions HTTP/1.1" {
                return Err(Failure {
                    status: 400,
                    exit: None,
                });
            }
            for l in lines {
                let (k, v) = l.split_once(':').ok_or(Failure {
                    status: 400,
                    exit: None,
                })?;
                if k.eq_ignore_ascii_case("transfer-encoding") {
                    return Err(Failure {
                        status: 400,
                        exit: None,
                    });
                }
                if k.eq_ignore_ascii_case("content-length") {
                    if length.is_some() {
                        return Err(Failure {
                            status: 400,
                            exit: None,
                        });
                    }
                    length = Some(v.trim().parse::<usize>().map_err(|_| Failure {
                        status: 400,
                        exit: None,
                    })?);
                }
            }
            if length.is_none() || length.unwrap() > LIMIT {
                return Err(Failure {
                    status: 400,
                    exit: None,
                });
            }
            boundary = Some(p + 4);
        }
        if let (Some(p), Some(n)) = (boundary, length)
            && bytes.len() >= p + n
        {
            if bytes.len() != p + n {
                return Err(Failure {
                    status: 400,
                    exit: None,
                });
            }
            return Ok((status, bytes[p..].to_vec()));
        }
    }
}
fn local(r: &Value, body: &Value, client: &TcpStream, timeout: u64) -> Result<ResultData, Failure> {
    let (host, addr) = routes::address(r["base"].as_str().unwrap()).map_err(|_| Failure {
        status: 502,
        exit: None,
    })?;
    let mut s =
        TcpStream::connect_timeout(&addr, Duration::from_millis(500)).map_err(|_| Failure {
            status: 503,
            exit: None,
        })?;
    s.set_read_timeout(Some(Duration::from_millis(50))).ok();
    s.set_write_timeout(Some(Duration::from_millis(500))).ok();
    let decide = r["kind"] == "decide";
    let (path, b) = if decide {
        let text = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("");
        let input: Value = serde_json::from_str(text).map_err(|_| Failure {
            status: 400,
            exit: None,
        })?;
        if [
            "contract",
            "contract_sha",
            "input",
            "source_ids",
            "tier",
            "deadline_ms",
        ]
        .iter()
        .any(|k| input.get(k).is_none())
        {
            return Err(Failure {
                status: 400,
                exit: None,
            });
        }
        // The design freezes the payload, but no HTTP path: require an explicit operator path.
        let path = r["path"].as_str().ok_or(Failure {
            status: 503,
            exit: None,
        })?;
        if !path.starts_with('/') || path.contains(['\r', '\n', ' ']) {
            return Err(Failure {
                status: 400,
                exit: None,
            });
        }
        (path.to_owned(), input)
    } else {
        let mut b = body.clone();
        b["model"] = r
            .get("model")
            .cloned()
            .unwrap_or_else(|| body["model"].clone());
        ("/v1/chat/completions".into(), b)
    };
    let data = serde_json::to_vec(&b).unwrap();
    let header = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    );
    s.write_all(header.as_bytes())
        .and_then(|_| s.write_all(&data))
        .map_err(|_| Failure {
            status: 502,
            exit: None,
        })?;
    let (status, bytes) = read_http(&mut s, Some(client), timeout)?;
    if status != 200 {
        return Err(Failure {
            status: if status == 503 { 503 } else { 502 },
            exit: None,
        });
    }
    let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| Failure {
        status: 502,
        exit: None,
    })?;
    if decide {
        let model = value
            .get("model")
            .and_then(|m| m.as_str().or_else(|| m["name"].as_str()))
            .map(str::to_owned);
        value = json!({"choices":[{"message":{"content":value.to_string()},"finish_reason":"stop"}],"model":model,"usage":{}});
    }
    Ok(ResultData {
        value,
        exit: None,
        honoured: !decide && body.get("max_tokens").is_some(),
        hash: None,
    })
}
fn budget(state: &Path, label: &str, r: &Value) -> Option<u64> {
    if !r["metered"].as_bool().unwrap_or(false) {
        return None;
    }
    let mut left = r["budget"].as_u64().unwrap();
    match fs::read(state.join("budget.json")) {
        Ok(b) => match serde_json::from_slice::<Value>(&b) {
            Ok(v) => {
                if let Some(n) = v[label].as_u64() {
                    left = left.min(n)
                }
            }
            Err(_) => return Some(0),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Some(0),
    }
    Some(left)
}
fn send(s: &mut TcpStream, status: u16, v: &Value) -> usize {
    let b = v.to_string();
    let h = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        match status {
            200 => "OK",
            400 => "Bad Request",
            429 => "Too Many Requests",
            503 => "Service Unavailable",
            504 => "Gateway Timeout",
            _ => "Bad Gateway",
        },
        b.len()
    );
    let _ = s
        .write_all(h.as_bytes())
        .and_then(|_| s.write_all(b.as_bytes()));
    b.len()
}
fn handle(
    mut s: TcpStream,
    routes: &routes::Routes,
    state: &Path,
    timeout: u64,
    ledger_lock: &std::sync::Mutex<()>,
    guard: Option<&money::Files>,
) {
    // The shared API layer, at its installed roots; every paid call goes through it.
    let layer = money::layer();
    let start = Instant::now();
    let ts = now();
    let id = format!(
        "{}-{}-{}",
        ts,
        std::process::id(),
        IDS.fetch_add(1, Ordering::Relaxed)
    );
    s.set_read_timeout(Some(Duration::from_millis(50))).ok();
    s.set_write_timeout(Some(Duration::from_millis(500))).ok();
    let mut status = 200;
    let mut body = json!({});
    let mut bytes_in = 0;
    match read_http(&mut s, None, timeout) {
        Ok((_, b)) => {
            bytes_in = b.len();
            match serde_json::from_slice(&b) {
                Ok(v) => body = v,
                Err(_) => status = 400,
            }
        }
        Err(e) => status = e.status,
    }
    // Only the bridge's four inputs cross the seam; unknown consumer fields are ignored.
    if let Some(obj) = body.as_object_mut() {
        obj.retain(|k, _| matches!(k.as_str(), "model" | "messages" | "max_tokens" | "stream"));
    }
    let mut reservation_guard = Some(ledger_lock.lock().unwrap_or_else(|e| e.into_inner()));
    let label = body["model"].as_str().unwrap_or("").to_owned();
    let route = routes.get(&label);
    let mut left = route.and_then(|r| budget(state, &label, r));
    let mut admitted: Option<stratum_api::Admitted> = None;
    let mut billing: Option<money::Billing> = None;
    let mut refusal: Option<String> = None;
    let mut generation = Value::Null;
    let mut usage = Value::Null;
    let mut completion = Value::Null;
    let mut child_exit = None;
    let mut hash = None;
    let mut honoured = false;
    let mut reported = json!("unknown");
    let mut finish = Value::Null;
    let mut reply = json!({"error":"route request failed"});
    if status == 200 {
        if state.join("STOP").exists() {
            status = 503
        } else if route.is_none()
            || body["stream"] == true
            || !body["messages"].is_array()
            || body.get("max_tokens").is_some_and(|v| v.as_u64().is_none())
        {
            status = 400
        } else if left == Some(0) {
            status = 429
        } else {
            let r = route.unwrap();
            // Every paid call goes through the shared layer: priced and checked against QROC's cap (and,
            // for a priced route, this batch's ceiling) before it is sent, settled and logged after.
            if paid::paid(r)
                && let Err(e) = &layer
            {
                status = 503;
                refusal = Some(e.clone());
            } else if paid::paid(r)
                && let Ok(layer) = &layer
            {
                let cap = r["max_output_tokens"].as_u64().or_else(|| {
                    layer
                        .route(&format!(
                            "{}:{}",
                            paid::provider(r),
                            r["model"].as_str().unwrap_or("")
                        ))
                        .ok()
                        .map(|lr| lr.max_output)
                });
                match cap {
                    None => {
                        status = 503;
                        refusal =
                            Some("no output cap for this paid route in the shared layer".into());
                    }
                    Some(cap) => {
                        let max_out = body["max_tokens"].as_u64().map_or(cap, |n| n.min(cap));
                        body["max_tokens"] = json!(max_out);
                        let inner =
                            guard
                                .filter(|_| routes::priced(r))
                                .map(|g| stratum_api::Inner {
                                    ceiling: g.ceiling.clone(),
                                    state: state.to_path_buf(),
                                });
                        let fp =
                            stratum_api::money::fingerprint(&label, &body["messages"], max_out);
                        let ask = stratum_api::Ask {
                            tool: money::TOOL,
                            id: Some(&id),
                            route: &label,
                            provider: paid::provider(r),
                            model: r["model"].as_str().unwrap_or(""),
                            input_bound: money::input_bound(&body["messages"]),
                            max_output: max_out,
                            fingerprint: &fp,
                            inner: inner.as_ref(),
                        };
                        if routes::priced(r) && guard.is_none() {
                            status = 503;
                            refusal = Some("no money guard configured".into());
                        } else {
                            match layer.admit(&ask) {
                                Ok(a) => admitted = Some(a),
                                Err(e) => {
                                    status = if ["unknown pricing", "no ceiling", "no cap"]
                                        .iter()
                                        .any(|p| e.reason.starts_with(p))
                                    {
                                        503
                                    } else {
                                        429
                                    };
                                    refusal = Some(e.reason);
                                }
                            }
                        }
                    }
                }
            }
            if status == 200 {
                left = left.map(|n| n - 1);
            }
            if let Some(n) = left.filter(|_| status == 200) {
                let ledger = state.join("budget.json");
                let mut v = fs::read(&ledger)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .unwrap_or(json!({}));
                v[label.as_str()] = json!(n);
                let pending = state.join("budget.pending");
                let saved = (|| -> std::io::Result<()> {
                    let mut f = OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(&pending)?;
                    f.write_all(v.to_string().as_bytes())?;
                    f.sync_all()?;
                    fs::rename(&pending, &ledger)?;
                    fs::File::open(state)?.sync_all()?;
                    Ok(())
                })();
                if saved.is_err() {
                    status = 502;
                    billing = Some(money::Billing::NotSent("call-cap ledger not saved".into()));
                }
            }
            drop(reservation_guard.take());
            if status == 200 {
                if r["kind"] == "codex" {
                    hash = codex::binary_hash(r["bin"].as_str().unwrap());
                }
                let result = match r["kind"].as_str().unwrap() {
                    "codex" => codex::invoke(r, &body, state, &id, &s, timeout),
                    "apikey" | "anthropic" if admitted.is_some() && layer.is_ok() => {
                        let Ok(layer) = &layer else {
                            unreachable!("checked in the guard")
                        };
                        let max_out = body["max_tokens"].as_u64().unwrap_or(0);
                        let (res, b) =
                            paid::invoke(layer, r, &body, max_out, state, &id, &s, timeout);
                        billing = Some(b);
                        res
                    }
                    "apikey" => apikey::invoke(r, &body, &s, timeout),
                    _ => local(r, &body, &s, timeout),
                };
                match result {
                    Ok(result) => {
                        child_exit = result.exit;
                        hash = result.hash.or(hash);
                        honoured = result.honoured;
                        let v = result.value;
                        generation = v
                            .get("id")
                            .filter(|g| g.is_string())
                            .cloned()
                            .unwrap_or(Value::Null);
                        usage = v
                            .get("usage")
                            .filter(|u| u.is_object())
                            .cloned()
                            .unwrap_or(Value::Null);
                        completion = v
                            .get("completion_reason")
                            .cloned()
                            .unwrap_or_else(|| v["choices"][0]["finish_reason"].clone());
                        if let Some(content) = v["choices"][0]["message"]["content"]
                            .as_str()
                            .or_else(|| v["choices"][0]["text"].as_str())
                        {
                            reported = v
                                .get("model")
                                .filter(|m| m.is_string())
                                .cloned()
                                .unwrap_or_else(|| json!("unknown"));
                            finish = json!(if v["choices"][0]["finish_reason"] == "length" {
                                "length"
                            } else {
                                "stop"
                            });
                            reply = json!({"choices":[{"message":{"content":content},"finish_reason":finish}],"model":reported,"route":label,"usage":v.get("usage").filter(|u|u.is_object()).cloned().unwrap_or(json!({}))});
                        } else {
                            status = 502
                        }
                    }
                    Err(e) => {
                        status = e.status;
                        child_exit = e.exit;
                    }
                }
            }
        }
    }
    if status != 200 {
        reply = json!({"error":match status{400=>"invalid request",429=>"route budget exhausted",503=>"route unavailable or stopped",504=>"provider timed out",_=>"provider failed"}});
        if let Some(why) = &refusal {
            reply["error"] = json!(why);
        }
    }
    drop(reservation_guard.take());
    let settled = match (&admitted, &layer) {
        (Some(a), Ok(layer)) => {
            let _settle_guard = ledger_lock.lock().unwrap_or_else(|e| e.into_inner());
            Some(
                layer.settle(
                    a,
                    billing
                        .take()
                        .unwrap_or(money::Billing::Unknown("no outcome recorded".into())),
                ),
            )
        }
        _ => None,
    };
    let charge = match (&admitted, &settled) {
        (Some(a), Some(st)) => {
            json!({"amount":st.charge.map(money::fmt).unwrap_or_else(||"unknown".into()),"currency":a.currency,"basis":st.basis})
        }
        _ => Value::Null,
    };
    if status == 200 {
        reply["generation_id"] = generation.clone();
        reply["completion_reason"] = completion.clone();
        reply["charge"] = charge.clone();
        // The output limit this reply was asked under after the route's cap: QROC records it.
        if honoured && let Some(n) = body["max_tokens"].as_u64() {
            reply["honoured_max_tokens"] = json!(n);
        }
        if let Some(a) = &admitted {
            reply["retry_of"] = json!(a.retry_of);
            reply["attempt"] = json!(a.attempt);
        }
    }
    let bytes_out = send(&mut s, status, &reply);
    let receipt = json!({"id":id,"ts":ts,"route":label,"kind":route.map(|r|r["kind"].clone()),"requested_model":route.and_then(|r|r.get("model")).unwrap_or(&body["model"]),"reported_model":reported,"binary_sha256":hash,"max_tokens":body["max_tokens"],"max_tokens_honoured":honoured,"finish_reason":finish,"bytes_in":bytes_in,"bytes_out":bytes_out,"status":status,"ms":start.elapsed().as_millis(),"child_exit":child_exit,"budget_left":left,"generation_id":generation,"served_model":reported,"usage":usage,"completion_reason":completion,"charge":charge,"reservation":admitted.as_ref().map(|a|json!({"amount":money::fmt(a.nano),"currency":a.currency,"input_bound_tokens":a.input_bound,"max_output_tokens":a.max_output,"price":a.price_key,"price_file_sha256":a.price_sha256})),"retry_of":admitted.as_ref().and_then(|a|a.retry_of.clone()),"attempt":admitted.as_ref().map(|a|a.attempt),"money_refusal":refusal,"settled_total":settled.as_ref().map(|t|money::fmt(t.view().0)),"reserved_total":settled.as_ref().map(|t|money::fmt(t.view().1))});
    let _record_guard = ledger_lock.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("receipts.jsonl"))
    {
        let _ = writeln!(f, "{receipt}");
        let _ = f.sync_data();
    }
}
struct ConnectionSlot(std::sync::Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>);
impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        let (count, available) = &*self.0;
        let mut active = count.lock().unwrap_or_else(|e| e.into_inner());
        *active -= 1;
        available.notify_one();
    }
}
fn arg(args: &[String], name: &str) -> Result<String, String> {
    args.windows(2)
        .find(|a| a[0] == name)
        .map(|a| a[1].clone())
        .ok_or(format!("missing {name}"))
}
/// A table with a priced route serves only with a readable price file pricing each such route and a
/// readable ceiling in the same currency; both are read again on every request.
/// A table with a priced route serves only when `--prices` names the shared layer's price table, which
/// prices each such route, and `--ceiling` is readable in the same currency; both are read again on
/// every request.
fn guard_files(
    a: &[String],
    r: &routes::Routes,
    layer: &stratum_api::Layer,
) -> Result<Option<money::Files>, String> {
    if !r.values().any(routes::priced) {
        return Ok(None);
    }
    let files = money::Files {
        prices: PathBuf::from(arg(a, "--prices").map_err(|_| "priced routes need --prices")?),
        ceiling: PathBuf::from(arg(a, "--ceiling").map_err(|_| "priced routes need --ceiling")?),
    };
    let prices = stratum_api::prices::load(&files.prices)?;
    money::check_prices(layer, &files.prices)?;
    let (_, cur) = money::load_ceiling(&files.ceiling)?;
    if cur != prices.currency {
        return Err("ceiling and price file currencies differ".into());
    }
    for (label, v) in r.iter().filter(|(_, v)| routes::priced(v)) {
        if prices
            .find(paid::provider(v), v["model"].as_str().unwrap_or(""))
            .is_none()
        {
            return Err(format!("no price for priced route {label}"));
        }
    }
    Ok(Some(files))
}
fn main() {
    if let Err(e) = entry() {
        eprintln!("route-shim: {e}");
        std::process::exit(2)
    }
}
fn entry() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str){
 Some("plan")=>{let r=routes::load(&arg(&a,"--routes")?)?;for (label,v) in r{println!("label={label} kind={} model={} metered={} budget={} priced={} max_output_tokens={}",v["kind"],v.get("model").unwrap_or(&json!(label)),v["metered"].as_bool().unwrap_or(false),v["budget"],routes::priced(&v),v["max_output_tokens"])}},
 Some("status")=>{let state=PathBuf::from(arg(&a,"--state")?);print!("{}",fs::read_to_string(state.join("receipts.jsonl")).unwrap_or_default());println!("remaining_budget={}",fs::read_to_string(state.join("budget.json")).unwrap_or_else(|_|"{}".into()));println!("{}",money::summary(&state));if let Ok(l)=money::layer(){println!("shared layer: {}",l.spend().map(|s|s.to_text().lines().find(|x|x.starts_with("qroc:")).unwrap_or("qroc: no spend yet").to_owned()).unwrap_or_else(|e|e.to_string()))}},
 Some("reconcile")=>{let state=PathBuf::from(arg(&a,"--state")?);let charge=money::amount(&json!(arg(&a,"--charge")?)).ok_or("--charge must be an exact non-negative amount")?;let (s,r)=money::reconcile(&state,&arg(&a,"--id")?,charge,&arg(&a,"--evidence")?)?;println!("reconciled settled={} reserved={}",money::fmt(s),money::fmt(r));},
 Some("serve")=>{let r=routes::load(&arg(&a,"--routes")?)?;let layer=money::layer()?;let guard=guard_files(&a,&r,&layer)?;let state=PathBuf::from(arg(&a,"--state")?);fs::create_dir_all(&state).map_err(|e|e.to_string())?;fs::set_permissions(&state,fs::Permissions::from_mode(0o700)).map_err(|e|e.to_string())?;let listen:SocketAddr=arg(&a,"--listen")?.parse().map_err(|_|"invalid listen")?;if !listen.ip().is_loopback()||listen.port()==8080{return Err("listen requires loopback, excludes 8080".into())}let l=TcpListener::bind(listen).map_err(|e|e.to_string())?;println!("LISTEN {}",l.local_addr().unwrap());std::io::stdout().flush().ok();let timeout=arg(&a,"--timeout-ms").ok().and_then(|s|s.parse().ok()).unwrap_or(120000);let max_concurrent=if a.iter().any(|s|s=="--max-concurrent"){arg(&a,"--max-concurrent")?.parse::<usize>().map_err(|_|"invalid max-concurrent")?}else{3};
 if max_concurrent==0{return Err("max-concurrent must be positive".into())}
 let r=std::sync::Arc::new(r);let state=std::sync::Arc::new(state);let guard=std::sync::Arc::new(guard);
 let ledger_lock=std::sync::Arc::new(std::sync::Mutex::new(()));
 let slots=std::sync::Arc::new((std::sync::Mutex::new(0usize),std::sync::Condvar::new()));
 for s in l.incoming(){let s=s.map_err(|e|e.to_string())?;
 let (count,available)=&*slots;
 let mut active=count.lock().unwrap_or_else(|e|e.into_inner());
 while *active>=max_concurrent{active=available.wait(active).unwrap_or_else(|e|e.into_inner());}
 *active+=1;drop(active);
 let r=std::sync::Arc::clone(&r);let state=std::sync::Arc::clone(&state);let ledger_lock=std::sync::Arc::clone(&ledger_lock);let slots=std::sync::Arc::clone(&slots);let guard=std::sync::Arc::clone(&guard);
 std::thread::spawn(move ||{let _slot=ConnectionSlot(slots);handle(s,&r,&state,timeout,&ledger_lock,guard.as_ref().as_ref())});
 }},
 _=>return Err("verbs: plan --routes FILE | status --state DIR | serve --listen ADDR --routes FILE --state DIR [--prices FILE --ceiling FILE] | reconcile --state DIR --id ID --charge AMOUNT --evidence TEXT".into())}
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/fixtures.rs"]
mod fixtures;
