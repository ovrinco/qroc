use serde_json::Value;
use std::{collections::BTreeMap, fs, net::SocketAddr};
pub type Routes = BTreeMap<String, Value>;
pub fn address(base: &str) -> Result<(String, SocketAddr), String> {
    let s = base
        .strip_prefix("http://")
        .ok_or("local base must use http://")?;
    let host = s.split('/').next().unwrap_or("");
    let addr: SocketAddr = host
        .parse()
        .map_err(|_| "local base requires a numeric loopback address and port")?;
    if !addr.ip().is_loopback() || addr.port() == 8080 {
        return Err("forbidden upstream address".into());
    }
    Ok((host.into(), addr))
}
/// A priced route is admitted by the money guard before its request leaves.
pub fn priced(r: &Value) -> bool {
    r["priced"].as_bool().unwrap_or(false)
}
pub fn load(path: &str) -> Result<Routes, String> {
    let v: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if v.to_string().contains(":8080") {
        return Err("port 8080 forbidden".into());
    }
    let obj = v["routes"].as_object().ok_or("missing routes object")?;
    let mut out = Routes::new();
    for (label, r) in obj {
        if label.is_empty() || !r.is_object() {
            return Err("invalid route".into());
        }
        // Reject the forbidden port anywhere, including otherwise unused fields.
        let raw = r.to_string();
        if raw.contains(":8080")
            || r.as_object()
                .unwrap()
                .values()
                .any(|v| v.as_u64() == Some(8080))
        {
            return Err("port 8080 forbidden".into());
        }
        let kind = r["kind"].as_str().ok_or("missing kind")?;
        if r["metered"].as_bool().unwrap_or(false) && r["budget"].as_u64().is_none() {
            return Err("metered route requires integer budget".into());
        }
        match kind {
            "codex" => {
                let b = r["bin"].as_str().ok_or("missing bin")?;
                if !b.starts_with('/')
                    || b.contains("/.local/bin/")
                    || b.starts_with("~/.local/bin")
                    || b.contains("/mise/shims/")
                {
                    return Err("unpinned binary path".into());
                }
                if r["model"].as_str().is_none() {
                    return Err("missing model".into());
                }
                if !matches!(
                    r["effort"].as_str(),
                    Some("low" | "medium" | "high" | "minimal" | "xhigh")
                ) {
                    return Err("invalid effort".into());
                }
            }
            "llama" | "decide" => {
                let (_, a) = address(r["base"].as_str().ok_or("missing base")?)?;
                if kind == "llama" && !(8081..=8092).contains(&a.port()) {
                    return Err("llama requires role port 8081-8092".into());
                }
                if kind == "decide" && a.port() != 8096 {
                    return Err("decide requires port 8096".into());
                }
            }
            "apikey" | "anthropic" => {
                let b = r["base"].as_str().ok_or("missing base")?;
                if !(b.starts_with("https://") || b.starts_with("http://"))
                    || b.contains(['\n', '\r', '"'])
                {
                    return Err("invalid API base".into());
                }
                if r["model"].as_str().is_none() {
                    return Err("missing API model/key_env".into());
                }
                // A paid route's key is read by the shared API layer from its one store; only a route
                // to a local stand-in on loopback names a key variable of its own.
                if !crate::paid::paid(r) && r["key_env"].as_str().is_none() {
                    return Err("missing API model/key_env".into());
                }
                if kind == "anthropic" && !priced(r) {
                    return Err("anthropic route must be priced".into());
                }
                if priced(r) {
                    if kind == "apikey" && r["provider"] != "openai" {
                        return Err("priced apikey route requires provider openai".into());
                    }
                    if !r["metered"].as_bool().unwrap_or(false) {
                        return Err("priced route must also be metered (call cap)".into());
                    }
                    if r["max_output_tokens"].as_u64().is_none_or(|n| n == 0) {
                        return Err("priced route requires positive max_output_tokens".into());
                    }
                    if r.get("effort").is_some()
                        && !matches!(
                            r["effort"].as_str(),
                            Some("low" | "medium" | "high" | "xhigh" | "max")
                        )
                    {
                        return Err("invalid effort".into());
                    }
                }
            }
            _ => return Err("unknown route kind".into()),
        }
        out.insert(label.clone(), r.clone());
    }
    Ok(out)
}
