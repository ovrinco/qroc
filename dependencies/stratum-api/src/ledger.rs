//! The layer's state: one account per tool (`accounts.json`: settled per day, every open or unresolved
//! reservation) and one append-only ledger for all tools (`ledger.jsonl`: every admission, refusal,
//! settlement and reconciliation with tool, route, provider, model, local date, reservation, charge,
//! basis and generation id). Both are changed only under the layer's file lock (`lock`), which holds
//! across processes; the accounts file is written and synced before a request leaves.
use crate::{Error, Layer, Result, caps, io_at, prices};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

pub const ACCOUNTS: &str = "accounts.json";
pub const LEDGER: &str = "ledger.jsonl";
pub const VERSION: &str = "stratum-api/1";

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The calendar day in local time of a moment (milliseconds since the epoch), as `YYYY-MM-DD`.
pub fn local_date(ms: u128) -> String {
    let secs = (ms / 1000).to_string();
    Command::new("/usr/bin/date")
        .args(["-d", &format!("@{secs}"), "+%Y-%m-%d"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| s.len() == 10)
        .unwrap_or_else(|| "unknown-day".into())
}

/// A moment (seconds since the epoch) in local time, as `YYYY-MM-DD HH:MM`.
pub fn local_minute(secs: u64) -> String {
    Command::new("/usr/bin/date")
        .args(["-d", &format!("@{secs}"), "+%Y-%m-%d %H:%M"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| s.len() == 16)
        .unwrap_or_else(|| "an unknown time".into())
}

/// The layer's home, made owner-only.
pub fn ensure_home(home: &Path) -> Result<()> {
    fs::create_dir_all(home).map_err(io_at(home))?;
    fs::set_permissions(home, fs::Permissions::from_mode(0o700)).map_err(io_at(home))
}

/// The cross-process lock around admission and settlement; released when dropped.
pub struct Lock(#[allow(dead_code)] File);

pub fn lock(home: &Path) -> Result<Lock> {
    ensure_home(home)?;
    let p = home.join("lock");
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(&p)
        .map_err(io_at(&p))?;
    f.lock().map_err(io_at(&p))?;
    Ok(Lock(f))
}

pub fn read_accounts(home: &Path) -> Result<Value> {
    let p = home.join(ACCOUNTS);
    match fs::read(&p) {
        Ok(b) => {
            let v: Value = serde_json::from_slice(&b)
                .map_err(|_| Error::Ledger("the accounts file is corrupt".into()))?;
            if v["version"] != VERSION {
                return Err(Error::Ledger(
                    "the accounts file has an unknown version".into(),
                ));
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(json!({"version":VERSION,"currency":null,"tools":{}}))
        }
        Err(e) => Err(Error::Io { path: p, source: e }),
    }
}

/// Write a JSON state file atomically and durably: a pending file synced, renamed, the folder synced.
pub fn write_synced(dir: &Path, name: &str, v: &Value) -> std::io::Result<()> {
    let pending = dir.join(format!("{name}.pending"));
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&pending)?;
    f.write_all(
        serde_json::to_string_pretty(v)
            .map_err(std::io::Error::other)?
            .as_bytes(),
    )?;
    f.sync_all()?;
    fs::rename(&pending, dir.join(name))?;
    File::open(dir)?.sync_all()
}

pub fn write_accounts(home: &Path, v: &Value) -> Result<()> {
    write_synced(home, ACCOUNTS, v)
        .map_err(|e| Error::Ledger(format!("the accounts file was not saved: {e}")))
}

/// Append one line to the ledger, synced.
pub fn append(home: &Path, line: &Value) {
    if ensure_home(home).is_err() {
        return;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(home.join(LEDGER))
    {
        let _ = writeln!(f, "{line}");
        let _ = f.sync_data();
    }
}

/// One tool's figures: settled, reserved (open and unresolved), unresolved count; for a day, the
/// same restricted to that day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub settled: u64,
    pub reserved: u64,
    pub unresolved: usize,
    pub unresolved_nano: u64,
}

pub fn totals(account: &Value, day: Option<&str>) -> Totals {
    let mut t = Totals::default();
    match day {
        None => t.settled = account["settled_nano"].as_u64().unwrap_or(0),
        Some(d) => t.settled = account["by_day"][d].as_u64().unwrap_or(0),
    }
    for r in account["reservations"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, r)| r)
    {
        if day.is_some_and(|d| r["day"] != d) {
            continue;
        }
        let n = r["nano"].as_u64().unwrap_or(u64::MAX);
        t.reserved = t.reserved.saturating_add(n);
        if r["state"] == "unresolved" {
            t.unresolved += 1;
            t.unresolved_nano = t.unresolved_nano.saturating_add(n);
        }
    }
    t
}

/// Spend as read by `stratum-api spend`.
#[derive(Debug, Clone)]
pub struct Spend {
    pub currency: String,
    pub tools: Vec<ToolSpend>,
}

#[derive(Debug, Clone)]
pub struct ToolSpend {
    pub tool: String,
    pub total: Totals,
    pub cap: Option<u64>,
    pub per_day_cap: Option<u64>,
    pub days: Vec<(String, Totals)>,
}

impl ToolSpend {
    pub fn left(&self) -> Option<u64> {
        self.cap
            .map(|c| c.saturating_sub(self.total.settled.saturating_add(self.total.reserved)))
    }
}

pub fn spend(layer: &Layer) -> Result<Spend> {
    let accounts = read_accounts(&layer.home)?;
    let caps = caps::load(&layer.caps_path()).ok();
    let mut names: Vec<String> = accounts["tools"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for (t, _) in caps.iter().flat_map(|c| c.tools.iter()) {
        if !names.contains(t) {
            names.push(t.clone());
        }
    }
    names.sort();
    let mut tools = Vec::new();
    for tool in names {
        let a = &accounts["tools"][&tool];
        let mut days: Vec<String> = a["by_day"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        for r in a["reservations"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, r)| r)
        {
            if let Some(d) = r["day"].as_str()
                && !days.iter().any(|x| x == d)
            {
                days.push(d.to_owned());
            }
        }
        days.sort();
        let cap = caps.as_ref().and_then(|c| c.get(&tool));
        tools.push(ToolSpend {
            tool: tool.clone(),
            total: totals(a, None),
            cap: cap.map(|c| c.total),
            per_day_cap: cap.and_then(|c| c.per_day),
            days: days
                .iter()
                .map(|d| (d.clone(), totals(a, Some(d))))
                .collect(),
        });
    }
    let currency = accounts["currency"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| caps.map(|c| c.currency))
        .unwrap_or_else(|| "USD".into());
    Ok(Spend { currency, tools })
}

impl Spend {
    pub fn to_json(&self) -> Value {
        let t = |x: &Totals| json!({"settled":prices::fmt(x.settled),"reserved":prices::fmt(x.reserved),"unresolved":x.unresolved,"unresolved_amount":prices::fmt(x.unresolved_nano)});
        json!({
            "currency": self.currency,
            "tools": self.tools.iter().map(|s| json!({
                "tool": s.tool,
                "total": t(&s.total),
                "cap": s.cap.map(prices::fmt),
                "left": s.left().map(prices::fmt),
                "per_day_cap": s.per_day_cap.map(prices::fmt),
                "days": s.days.iter().map(|(d, x)| {
                    let mut v = t(x);
                    v["day"] = json!(d);
                    v["left"] = json!(s.per_day_cap.map(|c| prices::fmt(c.saturating_sub(x.settled.saturating_add(x.reserved)))));
                    v
                }).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    /// The spend in plain words, one tool after another, each day beneath it.
    pub fn to_text(&self) -> String {
        let c = &self.currency;
        let p = prices::plain;
        let mut out = String::new();
        if self.tools.is_empty() {
            out.push_str("No tool has a cap or any spend yet.\n");
        }
        for s in &self.tools {
            out.push_str(&format!("{}: spent {} {c}", s.tool, p(s.total.settled)));
            let in_flight = s.total.reserved.saturating_sub(s.total.unresolved_nano);
            if in_flight > 0 {
                out.push_str(&format!(", held for calls in flight {} {c}", p(in_flight)));
            }
            if s.total.unresolved > 0 {
                out.push_str(&format!(
                    ", {} call(s) with an unknown charge holding {} {c} until reconciled",
                    s.total.unresolved,
                    p(s.total.unresolved_nano)
                ));
            }
            match (s.cap, s.left()) {
                (Some(cap), Some(left)) => {
                    out.push_str(&format!("; cap {} {c}, {} {c} left", p(cap), p(left)))
                }
                _ => out.push_str("; no cap, so every call is refused"),
            }
            if let Some(d) = s.per_day_cap {
                out.push_str(&format!("; at most {} {c} a day", p(d)));
            }
            out.push('\n');
            for (day, x) in &s.days {
                out.push_str(&format!("  {day}: spent {} {c}", p(x.settled)));
                if x.reserved > 0 {
                    out.push_str(&format!(", held {} {c}", p(x.reserved)));
                }
                if x.unresolved > 0 {
                    out.push_str(&format!(" ({} unknown)", x.unresolved));
                }
                if let Some(cap) = s.per_day_cap {
                    out.push_str(&format!(
                        ", {} {c} left that day",
                        p(cap.saturating_sub(x.settled.saturating_add(x.reserved)))
                    ));
                }
                out.push('\n');
            }
        }
        out
    }
}
