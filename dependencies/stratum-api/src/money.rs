//! The spend guard, lifted from QROC's route-shim `money.rs` and extended to one cap per tool.
//! Before a request leaves, a conservative upper bound of its charge is reserved from bounded input and
//! output sizes and the dated price table; it is admitted only when the tool's settled charges, every
//! open or unresolved reservation of that tool and the next reservation fit the tool's cap (and its
//! per-day cap, and the tool's own inner ceiling when it gives one: the tighter stop wins). The
//! reservation is written and synced before the request leaves. After the reply it is settled on the
//! billed usage × the dated price of the served model. An unknown charge (timeout, cut stream, missing
//! usage, server error, unpriced tier or model) stays reserved, and while one is unresolved nothing more
//! is admitted for that tool; `stratum-api reconcile` settles it with evidence. Prices and caps are
//! read again on every request. Amounts are nano-units of the currency (1e-9), kept as u64.
use crate::{
    Layer, Result, caps,
    ledger::{self, Totals},
    prices::{self, fmt},
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Fixed framing allowance per request and per message, in tokens, on top of one token per byte.
pub const FRAME: u64 = 512;
pub const PER_MESSAGE: u64 = 32;

/// Conservative upper bound on input tokens of OpenAI-form chat messages: one token per UTF-8 byte of
/// every message plus framing.
pub fn input_bound(messages: &Value) -> u64 {
    let mut n = FRAME;
    for m in messages.as_array().into_iter().flatten() {
        let content = match &m["content"] {
            Value::String(s) => s.len(),
            other => other.to_string().len(),
        };
        n += PER_MESSAGE + m["role"].as_str().map_or(0, str::len) as u64 + content as u64;
    }
    n
}

/// Conservative upper bound on input tokens of any request body: one token per byte of the whole
/// serialised body (instructions, tools and input items included) plus framing.
pub fn input_bound_bytes(body: &[u8]) -> u64 {
    FRAME + body.len() as u64
}

/// The fingerprint of a request, for retries: the same route, input and cap is a retry of the first.
pub fn fingerprint(route: &str, input: &Value, max_output: u64) -> String {
    prices::sha256(format!("{route}\n{input}\n{max_output}").as_bytes()).unwrap_or_default()
}

/// A tool's own inner ceiling, kept in its own folder in QROC's form (`money.json`, `money.log.jsonl`,
/// schema qroc-money/1) beside the tool's cap; both must admit.
#[derive(Debug, Clone, Default)]
pub struct Inner {
    pub ceiling: PathBuf,
    pub state: PathBuf,
}

/// One request asking to be admitted.
#[derive(Debug, Clone, Default)]
pub struct Ask<'a> {
    pub tool: &'a str,
    /// The caller's id for this request; the layer makes one when None.
    pub id: Option<&'a str>,
    pub route: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub input_bound: u64,
    pub max_output: u64,
    pub fingerprint: &'a str,
    pub inner: Option<&'a Inner>,
}

/// An admitted request: what was reserved and under which price, to be settled once.
#[derive(Debug, Clone)]
pub struct Admitted {
    pub tool: String,
    pub id: String,
    pub route: String,
    pub provider: String,
    pub model: String,
    pub day: String,
    pub nano: u64,
    pub input_bound: u64,
    pub max_output: u64,
    pub currency: String,
    pub price_key: String,
    pub price_sha256: Option<String>,
    pub retry_of: Option<String>,
    pub attempt: u64,
    pub inner: Option<Inner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    /// No price table, no price for the model, or currencies that differ.
    Pricing,
    /// The tool has no cap, or the cap or inner ceiling file cannot be read.
    NoCap,
    /// An earlier charge of this tool (or of its inner ceiling) is unknown.
    Unresolved,
    /// The tool's cap, its per-day cap or its inner ceiling would be passed.
    Cap,
    /// The layer's state could not be read or saved; nothing was sent.
    State,
}

/// A request refused before it was sent, with the reason in plain words.
#[derive(Debug, Clone)]
pub struct Refusal {
    pub kind: RefusalKind,
    pub reason: String,
    pub figures: Value,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl Refusal {
    /// The HTTP status a service front answers a refusal with (QROC's: pricing and missing cap 503,
    /// the rest 429).
    pub fn status(&self) -> u16 {
        match self.kind {
            RefusalKind::Pricing | RefusalKind::NoCap | RefusalKind::State => 503,
            RefusalKind::Unresolved | RefusalKind::Cap => 429,
        }
    }
}

/// How a request that was admitted ended, for its settlement.
#[derive(Debug, Clone)]
pub enum Billing {
    /// The provider replied with usage; served model as reported.
    Usage { reply: Value, served: String },
    /// Nothing left the machine (spawn or connect failed, or the request was never written).
    NotSent(String),
    /// The provider refused the request before generating, with this HTTP status and no usage.
    Rejected(u16),
    /// The charge cannot be known now: timeout, disconnect, server error, missing usage, unpriced reply.
    Unknown(String),
}

/// Provider statuses that mean the request was refused before any generation and is not billed.
pub fn not_billed(status: u16) -> bool {
    matches!(status, 400 | 401 | 403 | 404 | 413 | 422 | 429)
}

#[derive(Debug, Clone)]
pub struct Settled {
    pub charge: Option<u64>,
    pub basis: String,
    /// The tool's totals after settlement.
    pub tool: Totals,
    /// The inner ceiling's totals after settlement, when the request had one.
    pub inner: Option<Totals>,
}

impl Settled {
    /// Settled and reserved as the caller counts them: its inner ceiling's when it has one.
    pub fn view(&self) -> (u64, u64) {
        let t = self.inner.unwrap_or(self.tool);
        (t.settled, t.reserved)
    }
}

static IDS: AtomicU64 = AtomicU64::new(0);

fn new_id(ts: u128) -> String {
    format!(
        "{ts}-{}-{}",
        std::process::id(),
        IDS.fetch_add(1, Ordering::Relaxed)
    )
}

// ---- the inner ceiling, in QROC's own form (route-shim money.rs, unchanged) ----

pub const INNER_STATE: &str = "money.json";
pub const INNER_LOG: &str = "money.log.jsonl";
pub const INNER_VERSION: &str = "qroc-money/1";

/// The ceiling in nano-units and its currency: `{"ceiling":{"amount","currency"}}` or top-level fields.
pub fn load_ceiling(path: &Path) -> std::result::Result<(u64, String), String> {
    let v: Value = serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("ceiling file unreadable: {e}"))?,
    )
    .map_err(|_| "ceiling file is not JSON".to_string())?;
    let c = if v["ceiling"].is_object() {
        &v["ceiling"]
    } else {
        &v
    };
    let n = prices::amount(&c["amount"]).ok_or("ceiling amount missing or not exact")?;
    let cur = c["currency"]
        .as_str()
        .ok_or("ceiling currency missing")?
        .to_owned();
    Ok((n, cur))
}

fn read_inner(state: &Path) -> std::result::Result<Value, String> {
    match fs::read(state.join(INNER_STATE)) {
        Ok(b) => {
            let v: Value =
                serde_json::from_slice(&b).map_err(|_| "money state is corrupt".to_string())?;
            if v["version"] != INNER_VERSION {
                return Err("money state version unknown".into());
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(
            json!({"version":INNER_VERSION,"currency":null,"settled_nano":0,"reservations":{},"fingerprints":{}}),
        ),
        Err(e) => Err(format!("money state unreadable: {e}")),
    }
}

fn write_inner(state: &Path, v: &Value) -> std::result::Result<(), String> {
    let pending = state.join("money.pending");
    (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&pending)?;
        f.write_all(
            serde_json::to_string_pretty(v)
                .map_err(std::io::Error::other)?
                .as_bytes(),
        )?;
        f.sync_all()?;
        fs::rename(&pending, state.join(INNER_STATE))?;
        fs::File::open(state)?.sync_all()
    })()
    .map_err(|e| format!("money state not saved: {e}"))
}

pub fn inner_log(state: &Path, line: &Value) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join(INNER_LOG))
    {
        let _ = writeln!(f, "{line}");
        let _ = f.sync_data();
    }
}

fn inner_totals(v: &Value) -> Totals {
    let mut t = Totals {
        settled: v["settled_nano"].as_u64().unwrap_or(u64::MAX),
        ..Totals::default()
    };
    for r in v["reservations"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, r)| r)
    {
        let n = r["nano"].as_u64().unwrap_or(u64::MAX);
        t.reserved = t.reserved.saturating_add(n);
        if r["state"] == "unresolved" {
            t.unresolved += 1;
            t.unresolved_nano = t.unresolved_nano.saturating_add(n);
        }
    }
    t
}

/// A tool's inner ceiling's settled and reserved, in its own folder.
pub fn inner_figures(state: &Path) -> std::result::Result<(u64, u64), String> {
    let t = inner_totals(&read_inner(state)?);
    Ok((t.settled, t.reserved))
}

/// One line for a tool's own `status`: its inner ceiling's settled, reserved and unresolved.
pub fn inner_summary(state: &Path) -> String {
    match read_inner(state) {
        Ok(v) => {
            let t = inner_totals(&v);
            format!(
                "money currency={} settled={} reserved={} unresolved={}",
                v["currency"].as_str().unwrap_or("none"),
                fmt(t.settled),
                fmt(t.reserved),
                t.unresolved
            )
        }
        Err(e) => format!("money {e}"),
    }
}

// ---- admission ----

/// Reserve and admit one request, or refuse it unsent. Takes the layer's file lock.
pub fn admit(layer: &Layer, a: &Ask) -> std::result::Result<Admitted, Refusal> {
    let ts = ledger::now_ms();
    let day = ledger::local_date(ts);
    let id = a.id.map(str::to_owned).unwrap_or_else(|| new_id(ts));
    let base = json!({"ts":ts,"date":day,"tool":a.tool,"id":id,"route":a.route,"provider":a.provider,"model":a.model});
    let refuse = |kind: RefusalKind, reason: String, figures: Value| {
        let mut line = base.clone();
        line["decision"] = json!("refuse");
        line["reason"] = json!(reason);
        line["detail"] = figures.clone();
        ledger::append(&layer.home, &line);
        if let Some(i) = a.inner
            && fs::create_dir_all(&i.state).is_ok()
        {
            inner_log(
                &i.state,
                &json!({"ts":ts,"id":id,"route":a.route,"decision":"refuse","reason":reason,"detail":figures}),
            );
        }
        Err(Refusal {
            kind,
            reason,
            figures,
        })
    };
    if !caps::valid_tool(a.tool) {
        return refuse(
            RefusalKind::NoCap,
            format!("'{}' is not a valid tool name", a.tool),
            Value::Null,
        );
    }
    let _lock = match ledger::lock(&layer.home) {
        Ok(l) => l,
        Err(e) => return refuse(RefusalKind::State, format!("ledger lock: {e}"), Value::Null),
    };
    let prices = match prices::load(&layer.prices_path()) {
        Ok(p) => p,
        Err(e) => {
            return refuse(
                RefusalKind::Pricing,
                format!("unknown pricing: {e}"),
                Value::Null,
            );
        }
    };
    let Some(price) = prices.find(a.provider, a.model) else {
        return refuse(
            RefusalKind::Pricing,
            format!("unknown pricing: no price for {}:{}", a.provider, a.model),
            Value::Null,
        );
    };
    let caps = match caps::load(&layer.caps_path()) {
        Ok(c) => c,
        Err(e) => return refuse(RefusalKind::NoCap, format!("no cap: {e}"), Value::Null),
    };
    if caps.currency != prices.currency {
        return refuse(
            RefusalKind::Pricing,
            format!(
                "cap currency {} differs from price currency {}",
                caps.currency, prices.currency
            ),
            Value::Null,
        );
    }
    let Some(cap) = caps.get(a.tool).cloned() else {
        return refuse(
            RefusalKind::NoCap,
            format!("no cap: tool {} has no cap", a.tool),
            Value::Null,
        );
    };
    let cur = prices.currency.clone();
    let mut accounts = match ledger::read_accounts(&layer.home) {
        Ok(v) => v,
        Err(e) => return refuse(RefusalKind::State, e.to_string(), Value::Null),
    };
    if accounts["currency"].is_string() && accounts["currency"] != cur.as_str() {
        return refuse(
            RefusalKind::State,
            "the layer's accounts are kept in another currency".into(),
            Value::Null,
        );
    }
    let account = accounts["tools"][a.tool].clone();
    let all = ledger::totals(&account, None);
    let today = ledger::totals(&account, Some(&day));
    if all.unresolved > 0 {
        return refuse(
            RefusalKind::Unresolved,
            format!(
                "{} unresolved charge(s) await reconciliation",
                all.unresolved
            ),
            json!({"tool":a.tool,"settled":fmt(all.settled),"reserved":fmt(all.reserved),"cap":fmt(cap.total)}),
        );
    }
    let Some(next) = prices::reserve(price, a.input_bound, a.max_output) else {
        return refuse(RefusalKind::Cap, "reservation overflow".into(), Value::Null);
    };
    let total = all
        .settled
        .saturating_add(all.reserved)
        .saturating_add(next);
    let day_total = today
        .settled
        .saturating_add(today.reserved)
        .saturating_add(next);
    let mut figures = json!({"tool":a.tool,"settled":fmt(all.settled),"reserved":fmt(all.reserved),"next":fmt(next),"total":fmt(total),"cap":fmt(cap.total),"currency":cur,"input_bound":a.input_bound,"max_output":a.max_output});
    if let Some(d) = cap.per_day {
        figures["day"] = json!(day);
        figures["day_total"] = json!(fmt(day_total));
        figures["per_day_cap"] = json!(fmt(d));
    }
    if total > cap.total {
        return refuse(
            RefusalKind::Cap,
            format!("tool cap reached for {}", a.tool),
            figures,
        );
    }
    if cap.per_day.is_some_and(|d| day_total > d) {
        return refuse(
            RefusalKind::Cap,
            format!("daily cap reached for {}", a.tool),
            figures,
        );
    }
    // The tool's own inner ceiling, in QROC's form: same rules, its own folder.
    let mut inner_state = None;
    if let Some(i) = a.inner {
        let (ceiling, icur) = match load_ceiling(&i.ceiling) {
            Ok(c) => c,
            Err(e) => return refuse(RefusalKind::NoCap, format!("no ceiling: {e}"), Value::Null),
        };
        if icur != cur {
            return refuse(
                RefusalKind::Pricing,
                format!("ceiling currency {icur} differs from price currency {cur}"),
                Value::Null,
            );
        }
        let v = match read_inner(&i.state) {
            Ok(v) => v,
            Err(e) => return refuse(RefusalKind::State, e, Value::Null),
        };
        if v["currency"].is_string() && v["currency"] != cur.as_str() {
            return refuse(
                RefusalKind::State,
                "money state kept in another currency".into(),
                Value::Null,
            );
        }
        let t = inner_totals(&v);
        if t.unresolved > 0 {
            return refuse(
                RefusalKind::Unresolved,
                format!("{} unresolved charge(s) await reconciliation", t.unresolved),
                json!({"settled":fmt(t.settled),"reserved":fmt(t.reserved),"ceiling":fmt(ceiling)}),
            );
        }
        let itotal = t.settled.saturating_add(t.reserved).saturating_add(next);
        figures["inner"] = json!({"settled":fmt(t.settled),"reserved":fmt(t.reserved),"total":fmt(itotal),"ceiling":fmt(ceiling)});
        if itotal > ceiling {
            let qroc_figures = json!({"settled":fmt(t.settled),"reserved":fmt(t.reserved),"next":fmt(next),"total":fmt(itotal),"ceiling":fmt(ceiling),"currency":cur,"input_bound":a.input_bound,"max_output":a.max_output,"tool":figures});
            return refuse(
                RefusalKind::Cap,
                "spending ceiling reached".into(),
                qroc_figures,
            );
        }
        inner_state = Some(v);
    }
    // Retries: the same fingerprint again is a new reservation naming the first.
    let fp = a.fingerprint;
    let seen = match &inner_state {
        Some(v) => v["fingerprints"][fp].clone(),
        None => account["fingerprints"][fp].clone(),
    };
    let retry_of = seen["first"].as_str().map(str::to_owned);
    let attempt = seen["count"].as_u64().unwrap_or(0) + 1;
    let first = retry_of.clone().unwrap_or_else(|| id.clone());
    accounts["currency"] = json!(cur);
    let acc = &mut accounts["tools"][a.tool];
    if !acc.is_object() {
        *acc = json!({"settled_nano":0,"by_day":{},"reservations":{},"fingerprints":{}});
    }
    if !fp.is_empty() {
        acc["fingerprints"][fp] = json!({"first":first,"count":attempt});
    }
    acc["reservations"][&id] = json!({"route":a.route,"provider":a.provider,"model":a.model,"nano":next,"state":"open","ts":ts,"day":day,"attempt":attempt,"retry_of":retry_of,"inner_state":a.inner.map(|i| i.state.to_string_lossy().into_owned())});
    if let Err(e) = ledger::write_accounts(&layer.home, &accounts) {
        return refuse(RefusalKind::State, e.to_string(), Value::Null);
    }
    if let (Some(i), Some(mut v)) = (a.inner, inner_state) {
        v["currency"] = json!(cur);
        if !fp.is_empty() {
            v["fingerprints"][fp] = json!({"first":first,"count":attempt});
        }
        v["reservations"][&id] = json!({"route":a.route,"provider":a.provider,"model":a.model,"nano":next,"state":"open","ts":ts,"attempt":attempt,"retry_of":retry_of});
        if let Err(e) = write_inner(&i.state, &v) {
            // Take the tool's reservation back: nothing leaves.
            if let Some(o) = accounts["tools"][a.tool]["reservations"].as_object_mut() {
                o.remove(&id);
            }
            let _ = ledger::write_accounts(&layer.home, &accounts);
            return refuse(RefusalKind::State, e, Value::Null);
        }
        let mut qf = figures["inner"].clone();
        qf["next"] = json!(fmt(next));
        qf["currency"] = json!(cur);
        qf["input_bound"] = json!(a.input_bound);
        qf["max_output"] = json!(a.max_output);
        inner_log(
            &i.state,
            &json!({"ts":ts,"id":id,"route":a.route,"decision":"admit","reason":null,"detail":qf,"attempt":attempt,"retry_of":retry_of}),
        );
    }
    let mut line = base.clone();
    line["decision"] = json!("admit");
    line["reservation"] = json!(fmt(next));
    line["price"] = json!(price.key);
    line["detail"] = figures;
    line["attempt"] = json!(attempt);
    line["retry_of"] = json!(retry_of);
    ledger::append(&layer.home, &line);
    Ok(Admitted {
        tool: a.tool.to_owned(),
        id,
        route: a.route.to_owned(),
        provider: a.provider.to_owned(),
        model: a.model.to_owned(),
        day,
        nano: next,
        input_bound: a.input_bound,
        max_output: a.max_output,
        currency: cur,
        price_key: price.key.clone(),
        price_sha256: prices.sha256,
        retry_of,
        attempt,
        inner: a.inner.cloned(),
    })
}

// ---- settlement ----

/// The charge of how a request ended, and its basis in words.
fn price_billing(layer: &Layer, provider: &str, billing: &Billing) -> (Option<u64>, String, Value) {
    match billing {
        Billing::NotSent(why) => (Some(0), format!("not sent: {why}"), Value::Null),
        Billing::Rejected(s) => (
            Some(0),
            format!("not billed: provider refused before generation (HTTP {s})"),
            Value::Null,
        ),
        Billing::Unknown(why) => (None, format!("unknown: {why}"), Value::Null),
        Billing::Usage { reply, served } => {
            let generation = reply["id"].clone();
            match prices::load(&layer.prices_path()) {
                Err(e) => (None, format!("unknown: {e}"), generation),
                Ok(prices) => match prices.find(provider, served) {
                    None => (
                        None,
                        format!("unknown: served model '{served}' has no price"),
                        generation,
                    ),
                    Some(p) => match prices::charge(provider, p, reply) {
                        Ok(c) => (Some(c), format!("usage x {} price", p.key), generation),
                        Err(e) => (None, format!("unknown: {e}"), generation),
                    },
                },
            }
        }
    }
}

/// Settle one admitted request. Takes the layer's file lock.
pub fn settle(layer: &Layer, ad: &Admitted, billing: Billing) -> Settled {
    let (charge, basis, generation) = price_billing(layer, &ad.provider, &billing);
    let served = match &billing {
        Billing::Usage { served, .. } => json!(served),
        _ => Value::Null,
    };
    let lock = ledger::lock(&layer.home);
    let mut accounts = ledger::read_accounts(&layer.home).unwrap_or(Value::Null);
    if accounts.is_object()
        && accounts["tools"][&ad.tool]["reservations"]
            .get(&ad.id)
            .is_some()
    {
        let acc = &mut accounts["tools"][&ad.tool];
        match charge {
            Some(c) => {
                if let Some(o) = acc["reservations"].as_object_mut() {
                    o.remove(&ad.id);
                }
                acc["settled_nano"] =
                    json!(acc["settled_nano"].as_u64().unwrap_or(0).saturating_add(c));
                acc["by_day"][&ad.day] = json!(
                    acc["by_day"][&ad.day]
                        .as_u64()
                        .unwrap_or(0)
                        .saturating_add(c)
                );
            }
            None => {
                acc["reservations"][&ad.id]["state"] = json!("unresolved");
                acc["reservations"][&ad.id]["reason"] = json!(basis);
                acc["reservations"][&ad.id]["generation_id"] = generation.clone();
            }
        }
        let _ = ledger::write_accounts(&layer.home, &accounts);
    }
    let tool = ledger::totals(&accounts["tools"][&ad.tool], None);
    let inner = ad.inner.as_ref().map(|i| {
        let mut v = read_inner(&i.state).unwrap_or(Value::Null);
        if v.is_object() && v["reservations"].get(&ad.id).is_some() {
            match charge {
                Some(c) => {
                    if let Some(o) = v["reservations"].as_object_mut() {
                        o.remove(&ad.id);
                    }
                    v["settled_nano"] = json!(v["settled_nano"].as_u64().unwrap_or(0).saturating_add(c));
                }
                None => {
                    v["reservations"][&ad.id]["state"] = json!("unresolved");
                    v["reservations"][&ad.id]["reason"] = json!(basis);
                }
            }
            let _ = write_inner(&i.state, &v);
        }
        let t = inner_totals(&v);
        inner_log(&i.state, &json!({"id":ad.id,"decision":"settle","charge":charge.map(fmt).unwrap_or_else(|| "unknown".into()),"basis":basis,"settled":fmt(t.settled),"reserved":fmt(t.reserved)}));
        t
    });
    ledger::append(
        &layer.home,
        &json!({"ts":ledger::now_ms(),"date":ad.day,"tool":ad.tool,"id":ad.id,"route":ad.route,"provider":ad.provider,"model":ad.model,"served_model":served,"decision":"settle","reservation":fmt(ad.nano),"charge":charge.map(fmt).unwrap_or_else(|| "unknown".into()),"basis":basis,"generation_id":generation,"settled":fmt(tool.settled),"reserved":fmt(tool.reserved)}),
    );
    drop(lock);
    Settled {
        charge,
        basis,
        tool,
        inner,
    }
}

/// Settle an unresolved (or open) reservation of `tool` with an explicit charge and its evidence; its
/// inner ceiling, when it had one, is settled with it.
pub fn reconcile(
    layer: &Layer,
    tool: &str,
    id: &str,
    charge: u64,
    evidence: &str,
) -> Result<Totals> {
    if evidence.trim().is_empty() {
        return Err(crate::Error::Invalid("reconcile needs --evidence".into()));
    }
    let _lock = ledger::lock(&layer.home)?;
    let mut accounts = ledger::read_accounts(&layer.home)?;
    let acc = &mut accounts["tools"][tool];
    let r = acc["reservations"]
        .as_object_mut()
        .and_then(|o| o.remove(id))
        .ok_or_else(|| crate::Error::Invalid(format!("no reservation {id} for tool {tool}")))?;
    let day = r["day"].as_str().unwrap_or("unknown-day").to_owned();
    acc["settled_nano"] = json!(
        acc["settled_nano"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(charge)
    );
    acc["by_day"][&day] = json!(
        acc["by_day"][&day]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(charge)
    );
    ledger::write_accounts(&layer.home, &accounts)?;
    if let Some(state) = r["inner_state"].as_str() {
        let state = Path::new(state);
        if let Ok(mut v) = read_inner(state)
            && let Some(ir) = v["reservations"].as_object_mut().and_then(|o| o.remove(id))
        {
            v["settled_nano"] = json!(
                v["settled_nano"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(charge)
            );
            write_inner(state, &v).map_err(crate::Error::Ledger)?;
            let t = inner_totals(&v);
            inner_log(
                state,
                &json!({"id":id,"decision":"reconcile","charge":fmt(charge),"reserved_was":fmt(ir["nano"].as_u64().unwrap_or(0)),"evidence":evidence,"settled":fmt(t.settled),"reserved":fmt(t.reserved)}),
            );
        }
    }
    let t = ledger::totals(&accounts["tools"][tool], None);
    ledger::append(
        &layer.home,
        &json!({"ts":ledger::now_ms(),"date":day,"tool":tool,"id":id,"route":r["route"],"provider":r["provider"],"model":r["model"],"decision":"reconcile","reservation":fmt(r["nano"].as_u64().unwrap_or(0)),"charge":fmt(charge),"basis":format!("reconciled: {evidence}"),"generation_id":r["generation_id"],"settled":fmt(t.settled),"reserved":fmt(t.reserved)}),
    );
    Ok(t)
}

/// Reconcile a reservation known only to a tool's inner ceiling (one admitted before the layer), in
/// QROC's form: returns its settled and reserved after.
pub fn reconcile_inner(
    state: &Path,
    id: &str,
    charge: u64,
    evidence: &str,
) -> std::result::Result<(u64, u64), String> {
    if evidence.trim().is_empty() {
        return Err("reconcile needs --evidence".into());
    }
    let mut v = read_inner(state)?;
    let r = v["reservations"]
        .as_object_mut()
        .and_then(|o| o.remove(id))
        .ok_or(format!("no reservation {id}"))?;
    v["settled_nano"] = json!(
        v["settled_nano"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(charge)
    );
    write_inner(state, &v)?;
    let t = inner_totals(&v);
    inner_log(
        state,
        &json!({"id":id,"decision":"reconcile","charge":fmt(charge),"reserved_was":fmt(r["nano"].as_u64().unwrap_or(0)),"evidence":evidence,"settled":fmt(t.settled),"reserved":fmt(t.reserved)}),
    );
    Ok((t.settled, t.reserved))
}

/// Whether the layer holds a reservation `id` for `tool`.
pub fn holds(layer: &Layer, tool: &str, id: &str) -> bool {
    ledger::read_accounts(&layer.home)
        .is_ok_and(|v| v["tools"][tool]["reservations"].get(id).is_some())
}
