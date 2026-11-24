//! Append-only decision advice. No gate, scope, document or PHOD writes.
use crate::{Error, Result, registry};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const CONTRACT: &str = include_str!("../runtime/decide/contracts/qroc.gate4_consistency.v1.json");
const LIMIT: usize = 4 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn identity<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    let s = v[key]
        .as_str()
        .ok_or_else(|| invalid("Owner identity required"))?;
    if s.is_empty()
        || s.len() > 200
        || !s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(invalid("Invalid owner identity"));
    }
    Ok(s)
}
fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut p = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(invalid("Invalid store path"));
        }
        p.push(part);
        if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid("Advice/source path may not contain symlinks"));
        }
    }
    Ok(p)
}
fn read_value(p: &Path) -> Result<Value> {
    let mut b = Vec::new();
    fs::File::open(p)?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut b)?;
    if b.len() > LIMIT {
        return Err(invalid("Decision input exceeds bound"));
    }
    Ok(serde_json::from_slice(&b)?)
}
fn owned_sources(root: &Path, args: &Value, model: &Value) -> Result<BTreeMap<String, Value>> {
    let p = identity(args, "project_id")?;
    let b = identity(args, "branch_id")?;
    let project = read_value(&safe_path(
        root,
        Path::new(&format!("qroc/projects/{p}.json")),
    )?)?;
    let branch = read_value(&safe_path(
        root,
        Path::new(&format!("qroc/branches/{b}.json")),
    )?)?;
    if project["id"] != p || branch["id"] != b || branch["project_id"] != p {
        return Err(invalid("Decision owner mismatch"));
    }
    let kinds = model["reviewedDocuments"]
        .as_array()
        .ok_or_else(|| invalid("Missing source schema"))?;
    let revisions = args["revisions"]
        .as_object()
        .ok_or_else(|| invalid("Exact revisions map required"))?;
    if revisions.len() != kinds.len() {
        return Err(invalid("Exactly the schema-named revisions required"));
    }
    let mut owned = BTreeMap::new();
    for kind in kinds {
        let k = kind
            .as_str()
            .ok_or_else(|| invalid("Invalid schema document type"))?;
        let r = revisions
            .get(k)
            .and_then(Value::as_u64)
            .filter(|r| *r > 0 && *r <= u32::MAX as u64)
            .ok_or_else(|| invalid("Exact positive revision required"))?;
        let d = read_value(&safe_path(
            root,
            Path::new(&format!("qroc/documents/{p}/{b}/{k}-r{r}.json")),
        )?)?;
        let content = d["content"]
            .as_str()
            .ok_or_else(|| invalid("Missing source content"))?;
        if d["project_id"] != p
            || d["branch_id"] != b
            || d["document_type"] != k
            || d["revision"] != r
            || d["id"].as_str().is_none_or(str::is_empty)
            || d["sha256"] != digest(content.as_bytes())
        {
            return Err(invalid("Source identity/revision/digest mismatch"));
        }
        owned.insert(k.into(), d);
    }
    Ok(owned)
}
// Frozen transport signature. Only literal loopback HTTP; never port 8080.
fn post_chat_completion(
    endpoint: &str,
    body: &serde_json::Value,
    timeout_seconds: u64,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(serde_json::Value, String)> {
    let authority = endpoint
        .strip_prefix("http://")
        .ok_or_else(|| invalid("Loopback HTTP endpoint required"))?;
    let authority = authority
        .strip_suffix("/v1/chat/completions")
        .or_else(|| authority.strip_suffix('/'))
        .unwrap_or(authority);
    let addr: SocketAddr = authority
        .parse()
        .map_err(|_| invalid("Literal loopback address required"))?;
    if !addr.ip().is_loopback() || addr.port() == 8080 || addr.port() == 0 {
        return Err(invalid("Forbidden gateway endpoint"));
    }
    let duration = Duration::from_secs(timeout_seconds.clamp(1, 120));
    let deadline = Instant::now() + duration;
    if cancel.load(Ordering::Relaxed) {
        return Err(invalid("Cancelled"));
    }
    let bytes = serde_json::to_vec(body)?;
    if bytes.len() > LIMIT {
        return Err(invalid("Request exceeds bound"));
    }
    let mut stream = TcpStream::connect_timeout(&addr, duration.min(Duration::from_secs(3)))?;
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(duration.min(Duration::from_secs(3))))?;
    stream.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).as_bytes())?;
    stream.write_all(&bytes)?;
    let mut response = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            return Err(invalid("Cancelled or gateway timed out"));
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&chunk[..n]);
                if response.len() > LIMIT + 16384 {
                    return Err(invalid("Response exceeds bound"));
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
    let start = response
        .windows(4)
        .position(|b| b == b"\r\n\r\n")
        .map(|p| p + 4)
        .ok_or_else(|| invalid("Malformed HTTP"))?;
    if start > 16384 {
        return Err(invalid("Headers exceed bound"));
    }
    let headers =
        std::str::from_utf8(&response[..start]).map_err(|_| invalid("Invalid headers"))?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>();
    if status.len() < 2 || !matches!(status[0], "HTTP/1.0" | "HTTP/1.1") || status[1] != "200" {
        return Err(invalid("Gateway HTTP failure"));
    }
    let mut length = None;
    for line in lines.filter(|l| !l.is_empty()) {
        let (k, v) = line
            .split_once(':')
            .ok_or_else(|| invalid("Malformed header"))?;
        if k.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid("Unsupported transfer encoding"));
        }
        if k.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid("Duplicate content length"));
            }
            length = Some(
                v.trim()
                    .parse::<usize>()
                    .map_err(|_| invalid("Invalid content length"))?,
            );
        }
    }
    if length != Some(response.len() - start) || response.len() - start > LIMIT {
        return Err(invalid("Missing or mismatched response length"));
    }
    let v: Value = serde_json::from_slice(&response[start..])?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| invalid("Missing completion content"))?
        .to_owned();
    Ok((v, content))
}
fn assess(
    reply: &Value,
    content: &str,
    model: &Value,
    owned: &BTreeMap<String, Value>,
) -> (Vec<Value>, Vec<Value>, bool, bool, Option<String>) {
    let parsed = serde_json::from_str::<Value>(content);
    let answer = parsed.as_ref().ok();
    let mut error = if parsed.is_err() {
        Some("invalid_json".into())
    } else {
        None
    };
    let supplied = answer.and_then(|v| v["labels"].as_array());
    let rules = model["consistencyRules"]
        .as_array()
        .expect("validated schema");
    let mut labels = Vec::new();
    let mut spans = Vec::new();
    let mut all_verified = true;
    let mut any_abstained = false;
    let finish_ok = reply["choices"][0]["finish_reason"] == "stop";
    let identity_ok = reply["model"].as_str().is_some_and(|s| !s.is_empty())
        && reply["route"].as_str().is_some_and(|s| !s.is_empty());
    if !finish_ok {
        error = Some("incomplete_response".into());
    }
    if !identity_ok {
        error = Some("missing_provider_identity".into());
    }
    let shape_ok = answer.is_some_and(|v| {
        v.as_object()
            .is_some_and(|m| m.len() == 1 && m.contains_key("labels"))
    }) && supplied.is_some_and(|a| {
        a.len() == rules.len()
            && a.iter().all(|l| rules.iter().any(|r| r["id"] == l["id"]))
            && rules
                .iter()
                .all(|r| a.iter().filter(|l| l["id"] == r["id"]).count() == 1)
    });
    if !shape_ok && error.is_none() {
        error = Some("invalid_labels".into());
    }
    // Retain and check every supplied quote, including unknown or duplicate labels.
    if let Some(supplied) = supplied {
        for label in supplied {
            let rule = rules.iter().find(|r| r["id"] == label["id"]);
            if let Some(items) = label["evidence"].as_array() {
                for e in items {
                    let checked =
                        registry::verify_evidence(&json!([e]), &[e["force_ref"].clone()], owned)
                            .is_ok();
                    let valid = checked
                        && rule.is_some_and(|r| {
                            e["force_ref"] == r["left"] || e["force_ref"] == r["right"]
                        });
                    spans.push(json!({"rule_id":label["id"],"evidence":e,"quote_verified":valid}));
                }
            }
        }
    }
    for rule in rules {
        let refs = [rule["left"].clone(), rule["right"].clone()];
        let matches = supplied
            .map(|a| {
                a.iter()
                    .filter(|l| l["id"] == rule["id"])
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let label = if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        };
        let evidence = label.map(|l| l["evidence"].clone()).unwrap_or(json!([]));
        // Full expected reference set and every individual quote use the original verifier.
        let mut verified = registry::verify_evidence(&evidence, &refs, owned).is_ok();
        if let Some(items) = evidence.as_array() {
            for e in items {
                let valid =
                    registry::verify_evidence(&json!([e]), &[e["force_ref"].clone()], owned)
                        .is_ok()
                        && refs.contains(&e["force_ref"]);
                verified &= valid;
            }
        } else {
            verified = false;
        }
        let status = label
            .and_then(|l| l["label"].as_str())
            .filter(|s| matches!(*s, "supported" | "contradicted" | "insufficient"));
        let abstained = !verified
            || !finish_ok
            || !identity_ok
            || !shape_ok
            || status.is_none()
            || status == Some("insufficient");
        all_verified &= verified;
        any_abstained |= abstained;
        labels.push(json!({"id":rule["id"],"label":status.unwrap_or("insufficient"),"evidence":evidence,"quote_verified":verified,"abstained":abstained}));
    }
    (labels, spans, all_verified, any_abstained, error)
}
fn append(root: &Path, record: &mut Value) -> Result<()> {
    let dir = safe_path(root, Path::new("qroc/advice"))?;
    fs::create_dir_all(&dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("Invalid clock"))?
        .as_nanos();
    loop {
        let id = format!(
            "advice-{stamp}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        record["id"] = json!(id);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(format!("{id}.json")))
        {
            Ok(mut f) => {
                f.write_all(&serde_json::to_vec(record)?)?;
                f.write_all(b"\n")?;
                f.sync_all()?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

// Five read-only adapters share the frozen transport, verifier and append path.
fn profile_contract(command: &str) -> Option<&'static str> {
    match command {
        "qroc-decide-quote-support" => Some(include_str!(
            "../runtime/decide/contracts/qroc.quote_support.v1.json"
        )),
        "qroc-decide-scope-readiness" => Some(include_str!(
            "../runtime/decide/contracts/qroc.scope_readiness.v1.json"
        )),
        "qroc-decide-source-class" => Some(include_str!(
            "../runtime/decide/contracts/qroc.source_class.v1.json"
        )),
        "qroc-decide-duplicates" => Some(include_str!(
            "../runtime/decide/contracts/qroc.duplicates.v1.json"
        )),
        "qroc-decide-phod-readiness" => Some(include_str!(
            "../runtime/decide/contracts/qroc.phod_readiness.v1.json"
        )),
        _ => None,
    }
}
fn proposal_valid(profile: &str, label: &Value) -> bool {
    if !label.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            [
                "id",
                "label",
                "evidence",
                "included_types",
                "excluded_types",
                "ambiguity_category",
            ]
            .contains(&k.as_str())
        })
    }) {
        return false;
    }
    match profile {
        "qroc.scope_readiness.v1" => {
            ["included_types", "excluded_types"].iter().all(|k| {
                label[*k].as_array().is_some_and(|a| {
                    a.len() <= 100
                        && a.iter().all(|v| {
                            v.as_str().is_some_and(|s| {
                                !s.is_empty()
                                    && s.len() <= 100
                                    && crate::qroc::normalized_document(s).is_ok()
                            })
                        })
                })
            }) && label["included_types"].as_array().is_some_and(|a| {
                a.iter()
                    .all(|v| !label["excluded_types"].as_array().unwrap().contains(v))
            })
        }
        "qroc.phod_readiness.v1" if label["label"] == "ambiguous" => {
            label["ambiguity_category"].as_str().is_some_and(|c| {
                [
                    "required behaviour",
                    "input/data shape",
                    "units/ranges",
                    "state/lifecycle",
                    "effects/capabilities",
                    "target/environment assumptions",
                    "error behaviour",
                    "nondeterminism",
                    "verification strength",
                    "competing preferences",
                ]
                .contains(&c)
            })
        }
        _ => true,
    }
}
fn profile_advice(root: &Path, args: &Value, contract: &str) -> Result<Value> {
    identity(args, "project_id")?;
    identity(args, "branch_id")?;
    owned_sources(
        root,
        &json!({"project_id":args["project_id"],"branch_id":args["branch_id"],"revisions":{}}),
        &json!({"reviewedDocuments":[]}),
    )?;
    let specification: Value = serde_json::from_str(contract)?;
    let profile = specification["contract"].as_str().unwrap();
    let subjects = args["subjects"]
        .as_array()
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .ok_or_else(|| invalid("1..64 advice subjects required"))?;
    let mut contexts = Vec::new();
    let mut owners = Vec::new();
    let mut hashes = serde_json::Map::new();
    let mut ids = std::collections::BTreeSet::new();
    for subject in subjects {
        let id = identity(subject, "id")?;
        if !ids.insert(id) {
            return Err(invalid("Duplicate advice subject"));
        }
        let sources = subject["sources"]
            .as_array()
            .filter(|a| {
                a.len()
                    == if profile == "qroc.duplicates.v1" {
                        2
                    } else {
                        1
                    }
            })
            .ok_or_else(|| invalid("Wrong profile source owner count"))?;
        let mut context_sources = Vec::new();
        let mut subject_owners = Vec::new();
        for (i, source) in sources.iter().enumerate() {
            let refs = source["refs"]
                .as_array()
                .filter(|a| !a.is_empty() && a.len() <= 64)
                .ok_or_else(|| invalid("Source references required"))?;
            let kinds = source["revisions"]
                .as_object()
                .ok_or_else(|| invalid("Exact source revisions required"))?
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            if kinds.is_empty()
                || kinds.len() > 32
                || refs.iter().any(|r| {
                    !kinds.iter().any(|k| r["documentType"] == *k)
                        || r["forceId"].as_str().is_none_or(str::is_empty)
                })
            {
                return Err(invalid("Invalid source references"));
            }
            let source_args = json!({"project_id":args["project_id"],"branch_id":source.get("branch_id").unwrap_or(&args["branch_id"]),"revisions":source["revisions"]});
            let owned = owned_sources(root, &source_args, &json!({"reviewedDocuments":kinds}))?;
            for (kind, d) in &owned {
                hashes.insert(format!("{id}/{i}/{kind}"), d["sha256"].clone());
            }
            if profile == "qroc.quote_support.v1" {
                let m = registry::semantic_model("GATE_4")?
                    .ok_or_else(|| invalid("Missing gate model"))?;
                let c = m["criteria"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == subject["criterion_id"])
                    .ok_or_else(|| invalid("Unknown gate criterion"))?;
                let expected = c["derivedFromForces"].as_array().unwrap();
                if refs.len() != expected.len()
                    || expected
                        .iter()
                        .any(|r| refs.iter().filter(|v| *v == r).count() != 1)
                    || subject["claim"].as_str().is_none_or(str::is_empty)
                {
                    return Err(invalid("Criterion references and claim required"));
                }
            }
            if profile == "qroc.source_class.v1"
                && subject["span"].as_str().is_none_or(|span| {
                    span.is_empty()
                        || !owned
                            .values()
                            .any(|d| d["content"].as_str().unwrap().contains(span))
                })
            {
                return Err(invalid("Imported span must belong to exact revision"));
            }
            context_sources
                .push(json!({"branch_id":source_args["branch_id"],"refs":refs,"documents":owned}));
            subject_owners.push((owned, refs.clone()));
        }
        if profile == "qroc.duplicates.v1"
            && (context_sources[0]["branch_id"] == context_sources[1]["branch_id"]
                || sources[0]["revisions"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>()
                    != sources[1]["revisions"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .collect::<Vec<_>>())
        {
            return Err(invalid(
                "Duplicate comparison requires different branches and same document types",
            ));
        }
        contexts.push(json!({"id":id,"claim":subject["claim"],"criterion_id":subject["criterion_id"],"span":subject["span"],"sources":context_sources}));
        owners.push(subject_owners);
    }
    let endpoint = match std::env::var("QROC_GATEWAY") {
        Ok(e) => e,
        Err(_) => return Ok(json!({"error":"unavailable"})),
    };
    let body = json!({"model":format!("decide:{}",profile.strip_prefix("qroc.").unwrap().strip_suffix(".v1").unwrap()),"stream":false,"max_tokens":2048,"response_format":{"type":"json_object"},"messages":[{"role":"system","content":contract},{"role":"user","content":serde_json::to_string(&json!({"consumer":"qroc","profile":profile,"subjects":contexts}))?}]});
    let (reply, content) = match post_chat_completion(&endpoint, &body, 30, &AtomicBool::new(false))
    {
        Ok(v) => v,
        Err(_) => return Ok(json!({"error":"unavailable"})),
    };
    let answer = serde_json::from_str::<Value>(&content).ok();
    let supplied = answer.as_ref().and_then(|v| v["labels"].as_array());
    let shape = answer.as_ref().is_some_and(|v| {
        v.as_object()
            .is_some_and(|o| o.len() == 1 && o.contains_key("labels"))
    }) && supplied.is_some_and(|a| {
        a.len() == subjects.len()
            && a.iter()
                .all(|l| ids.contains(l["id"].as_str().unwrap_or("")))
            && ids
                .iter()
                .all(|id| a.iter().filter(|l| l["id"] == *id).count() == 1)
    });
    let identity_ok = reply["model"].as_str().is_some_and(|s| !s.is_empty())
        && reply["route"].as_str().is_some_and(|s| !s.is_empty())
        && reply
            .get("receipt_id")
            .or_else(|| reply.get("id"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
    let mut spans = Vec::new();
    let mut labels = Vec::new();
    let mut verified = shape;
    let mut abstained = !shape || !identity_ok || reply["choices"][0]["finish_reason"] != "stop";
    // Verify every supplied quote, including unknown/duplicate labels and extra groups.
    if let Some(items) = supplied {
        for label in items {
            let owner = subjects.iter().position(|s| s["id"] == label["id"]);
            if let Some(groups) = label["evidence"].as_array() {
                for (i, group) in groups.iter().enumerate() {
                    if let Some(es) = group.as_array() {
                        for e in es {
                            let valid =
                                owner
                                    .and_then(|n| owners[n].get(i))
                                    .is_some_and(|(o, refs)| {
                                        refs.contains(&e["force_ref"])
                                            && registry::verify_evidence(
                                                &json!([e]),
                                                &[e["force_ref"].clone()],
                                                o,
                                            )
                                            .is_ok()
                                    });
                            verified &= valid;
                            spans.push(json!({"subject_id":label["id"],"source_index":i,"evidence":e,"quote_verified":valid}));
                        }
                    } else {
                        verified = false;
                    }
                }
            } else {
                verified = false;
            }
        }
    }
    let response_abstained = abstained;
    for (n, subject) in subjects.iter().enumerate() {
        let found = supplied.and_then(|a| a.iter().find(|l| l["id"] == subject["id"]));
        let mut label = found
            .cloned()
            .unwrap_or(json!({"id":subject["id"],"label":"insufficient","evidence":[]}));
        let evidence = label["evidence"].as_array();
        let valid = evidence.is_some_and(|groups| {
            groups.len() == owners[n].len()
                && groups
                    .iter()
                    .zip(&owners[n])
                    .all(|(g, (o, r))| registry::verify_evidence(g, r, o).is_ok())
        }) && (profile != "qroc.source_class.v1"
            || evidence.is_some_and(|groups| {
                groups.iter().all(|g| {
                    g.as_array()
                        .is_some_and(|a| a.iter().all(|e| e["quote"] == subject["span"]))
                })
            }));
        let known = specification["questions"]["advice"]["labels"]
            .as_array()
            .unwrap()
            .contains(&label["label"]);
        let ab = response_abstained
            || !valid
            || !known
            || !proposal_valid(profile, &label)
            || label["label"] == "insufficient";
        verified &= valid;
        abstained |= ab;
        label["quote_verified"] = json!(valid);
        label["abstained"] = json!(ab);
        labels.push(label);
    }
    abstained |= !verified;
    let mut record = json!({"schema":"qroc-advice/1","receipt_id":reply.get("receipt_id").or_else(||reply.get("id")),"profile":profile,"contract_sha256":digest(contract.as_bytes()),"consumer":"qroc","project_id":args["project_id"],"branch_id":args["branch_id"],"revision_sha256s":hashes,"subjects":subjects,"model":reply["model"],"route":reply["route"],"labels":labels,"supporting_spans":spans,"quote_verified":verified,"abstained":abstained,"error":if answer.is_none(){Some("invalid_json")}else if !shape{Some("invalid_labels")}else{None},"advisory":true});
    append(root, &mut record)?;
    Ok(record)
}

pub fn command(root: &Path, command: &str, args: &Value) -> Result<Value> {
    if let Some(contract) = profile_contract(command) {
        return profile_advice(root, args, contract);
    }
    match command {
        "qroc-decide-list" => {
            let p = identity(args, "project_id")?;
            let b = identity(args, "branch_id")?;
            let dir = safe_path(root, Path::new("qroc/advice"))?;
            let mut advice = Vec::new();
            if dir.exists() {
                for item in fs::read_dir(dir)? {
                    let item = item?;
                    if !item.file_type()?.is_file()
                        || item.path().extension().is_none_or(|e| e != "json")
                    {
                        continue;
                    }
                    let v = read_value(&item.path())?;
                    if v["project_id"] == p && v["branch_id"] == b {
                        advice.push(v);
                    }
                }
            }
            advice.sort_by_key(|v| v["id"].as_str().unwrap_or("").to_owned());
            Ok(json!({"advice":advice}))
        }
        "qroc-decide-gate4-consistency" => {
            let model = registry::semantic_model("GATE_4")?
                .ok_or_else(|| invalid("Missing GATE_4 model"))?;
            let owned = owned_sources(root, args, &model)?;
            let endpoint = match std::env::var("QROC_GATEWAY") {
                Ok(e) => e,
                Err(_) => return Ok(json!({"error":"unavailable"})),
            };
            let body = json!({"model":"decide:gate4_consistency","stream":false,"max_tokens":2048,"response_format":{"type":"json_object"},"messages":[{"role":"system","content":CONTRACT},{"role":"user","content":serde_json::to_string(&json!({"consumer":"qroc","profile":"qroc.gate4_consistency.v1","project_id":args["project_id"],"branch_id":args["branch_id"],"rules":model["consistencyRules"],"sources":owned}))?}]});
            let (reply, content) =
                match post_chat_completion(&endpoint, &body, 30, &AtomicBool::new(false)) {
                    Ok(v) => v,
                    Err(_) => return Ok(json!({"error":"unavailable"})),
                };
            let (labels, spans, verified, abstained, error) =
                assess(&reply, &content, &model, &owned);
            let hashes = owned
                .iter()
                .map(|(k, v)| (k.clone(), v["sha256"].clone()))
                .collect::<serde_json::Map<_, _>>();
            let mut record = json!({"schema":"qroc-advice/1","receipt_id":reply.get("receipt_id").or_else(||reply.get("id")),"profile":"qroc.gate4_consistency.v1","contract_sha256":digest(CONTRACT.as_bytes()),"consumer":"qroc","project_id":args["project_id"],"branch_id":args["branch_id"],"revision_sha256s":hashes,"revisions":args["revisions"],"model":reply["model"],"route":reply["route"],"labels":labels,"supporting_spans":spans,"quote_verified":verified,"abstained":abstained,"error":error,"advisory":true});
            append(root, &mut record)?;
            Ok(record)
        }
        _ => Err(invalid("Unknown decision command")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    fn fixture() -> (Value, BTreeMap<String, Value>, Value) {
        let model = registry::semantic_model("GATE_4").unwrap().unwrap();
        let mut owned = BTreeMap::new();
        for k in model["reviewedDocuments"].as_array().unwrap() {
            let k = k.as_str().unwrap();
            owned.insert(k.into(),json!({"id":k,"revision":7,"sha256":"fixture-digest","content":"Exact source quotation."}));
        }
        let labels=model["consistencyRules"].as_array().unwrap().iter().map(|r| {
            let evidence=[r["left"].clone(),r["right"].clone()].iter().map(|f| {
                let d=&owned[f["documentType"].as_str().unwrap()];
                json!({"force_ref":f,"document_id":d["id"],"revision":d["revision"],"sha256":d["sha256"],"quote":"Exact source quotation."})
            }).collect::<Vec<_>>();
            json!({"id":r["id"],"label":"supported","evidence":evidence})
        }).collect::<Vec<_>>();
        (model, owned, json!({"labels":labels}))
    }
    fn envelope(content: &str) -> Value {
        json!({"model":"fixture-actual","route":"fixture","choices":[{"finish_reason":"stop","message":{"content":content}}]})
    }
    #[test]
    fn fake_loopback_transport_and_verifier() {
        let (m, o, a) = fixture();
        let e = envelope(&a.to_string());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = s.read(&mut buf).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&buf[..n]);
                if let Some(p) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..p]);
                    let len = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    if request.len() >= p + 4 + len {
                        let b: Value = serde_json::from_slice(&request[p + 4..]).unwrap();
                        assert_eq!(b["model"], "decide:gate4_consistency");
                        break;
                    }
                }
            }
            let b = serde_json::to_vec(&e).unwrap();
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                b.len()
            )
            .unwrap();
            s.write_all(&b).unwrap();
        });
        let (e, s) = post_chat_completion(
            &url,
            &json!({"model":"decide:gate4_consistency"}),
            2,
            &AtomicBool::new(false),
        )
        .unwrap();
        t.join().unwrap();
        let (l, sp, v, a, error) = assess(&e, &s, &m, &o);
        assert!(v);
        assert!(!a);
        assert!(error.is_none());
        assert_eq!(l.len(), 2);
        assert_eq!(sp.len(), 4);
    }
    #[test]
    fn wrong_quote_revision_and_missing_reference_abstain() {
        let (m, o, a) = fixture();
        for key in ["quote", "revision", "sha256", "document_id"] {
            let mut bad = a.clone();
            bad["labels"][0]["evidence"][0][key] = json!("not verbatim or not owner");
            let (_, _, v, ab, _) = assess(&envelope(&bad.to_string()), &bad.to_string(), &m, &o);
            assert!(!v);
            assert!(ab);
        }
        let mut missing = a.clone();
        missing["labels"][0]["evidence"] = json!([]);
        let (_, _, v, ab, _) = assess(
            &envelope(&missing.to_string()),
            &missing.to_string(),
            &m,
            &o,
        );
        assert!(!v);
        assert!(ab);
    }
    #[test]
    fn duplicate_unknown_missing_labels_and_truncation_abstain() {
        let (m, o, a) = fixture();
        let mut dup = a.clone();
        dup["labels"][1] = dup["labels"][0].clone();
        let mut unknown = a.clone();
        unknown["labels"][0]["id"] = json!("unknown");
        for bad in [
            dup,
            unknown,
            json!({"labels":[]}),
            json!({"labels":a["labels"],"authority":"passed"}),
        ] {
            let (_, _, _, ab, err) = assess(&envelope(&bad.to_string()), &bad.to_string(), &m, &o);
            assert!(ab);
            assert!(err.is_some());
        }
        let mut e = envelope(&a.to_string());
        e["choices"][0]["finish_reason"] = json!("length");
        assert!(assess(&e, &a.to_string(), &m, &o).3);
        let mut e = envelope(&a.to_string());
        e["model"] = Value::Null;
        assert!(assess(&e, &a.to_string(), &m, &o).3);
        assert!(assess(&envelope("not json"), "not json", &m, &o).3);
    }
    #[test]
    fn forbidden_endpoints_and_cancellation() {
        for e in [
            "http://127.0.0.1:8080",
            "http://192.0.2.1:1234",
            "https://127.0.0.1:1234",
            "http://localhost:1234",
            "http://127.0.0.1:0",
        ] {
            assert!(post_chat_completion(e, &json!({}), 1, &AtomicBool::new(false)).is_err());
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(
            post_chat_completion(
                &format!("http://{addr}"),
                &json!({}),
                1,
                &AtomicBool::new(true)
            )
            .is_err()
        );
    }
    #[test]
    fn append_preserves_existing_bytes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("roots")
            .join(format!(
                "unit-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let mut a = json!({"advisory":true});
        append(&root, &mut a).unwrap();
        let path = root
            .join("qroc/advice")
            .join(format!("{}.json", a["id"].as_str().unwrap()));
        let before = fs::read(&path).unwrap();
        let mut b = json!({"advisory":true});
        append(&root, &mut b).unwrap();
        assert_ne!(a["id"], b["id"]);
        assert_eq!(before, fs::read(path).unwrap());
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[test]
    fn profiles_have_advisory_contracts_and_bounded_choices() {
        for command in [
            "quote-support",
            "scope-readiness",
            "source-class",
            "duplicates",
            "phod-readiness",
        ] {
            let c: Value =
                serde_json::from_str(profile_contract(&format!("qroc-decide-{command}")).unwrap())
                    .unwrap();
            assert_eq!(c["authority"], "advisory only; no status changes");
            assert!(
                c["questions"]["advice"]["labels"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("insufficient"))
            );
        }
    }
    #[test]
    fn ambiguity_requires_named_category_and_scope_is_proposal() {
        assert!(!proposal_valid(
            "qroc.phod_readiness.v1",
            &json!({"label":"ambiguous"})
        ));
        assert!(proposal_valid(
            "qroc.phod_readiness.v1",
            &json!({"label":"ambiguous","ambiguity_category":"units/ranges"})
        ));
        assert!(!proposal_valid(
            "qroc.scope_readiness.v1",
            &json!({"included_types":["BRIEF"],"excluded_types":["BRIEF"]})
        ));
    }
}
