//! The image route: OpenAI's Image API (`/v1/images/generations`, and `/v1/images/edits` with
//! reference images) under the same guard as every other call. Before a request leaves, its most
//! possible cost is reserved: the prompt at one token per byte plus framing and every reference image
//! at the table's per-image bound, at the dearer of the text and image input rates, plus the output
//! tokens OpenAI's own rule gives for the requested size and quality, times `n`, at the image output
//! rate. Size and quality must therefore be named (`auto` cannot be bounded), and streaming and partial
//! images are not offered. After the reply it is settled on the reply's `usage` at the dated price.
//! The service front is `POST /tool/<tool>/v1/images/generations` and `.../v1/images/edits`; the
//! library front is [`run`]; `stratum-api image` is a thin caller of [`run`] for scripts.
use crate::{
    Ask, Billing, Layer,
    chat::{self, Route},
    keys, money,
    prices::{self, fmt},
    serve::{disconnected, provider_name, respond},
};
use serde_json::{Value, json};
use std::{net::TcpStream, path::Path};

/// The route a call without a route label of its own takes.
pub const DEFAULT_ROUTE: &str = "images:openai";
/// The largest request body the service reads on the edits path (reference images ride in it).
pub const REQUEST_LIMIT: usize = 128 * 1024 * 1024;
/// The largest reply kept (the images come back whole, base64-encoded).
pub const REPLY_LIMIT: usize = 256 * 1024 * 1024;
/// The Image API's own limits: images per call and reference images per edit.
pub const MAX_N: u64 = 10;
pub const MAX_IMAGES: usize = 16;
/// Tokens added per image on top of OpenAI's output rule, whose figures may differ from billing by
/// rounding.
pub const OUTPUT_SLACK: u64 = 16;

const GENERATE_FIELDS: &[&str] = &[
    "model",
    "prompt",
    "n",
    "size",
    "quality",
    "output_format",
    "output_compression",
    "background",
    "moderation",
    "user",
];
const EDIT_FIELDS: &[&str] = &["images", "mask", "markup", "input_fidelity"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Generate,
    Edit,
}

impl Kind {
    pub fn endpoint(self) -> &'static str {
        match self {
            Kind::Generate => "v1/images/generations",
            Kind::Edit => "v1/images/edits",
        }
    }
}

/// A size `WIDTHxHEIGHT` under the Image API's rules: both sides multiples of 16, at least 655,360 and
/// at most 8,294,400 pixels, no edge above 3840, the long side at most three times the short.
pub fn parse_size(s: &str) -> std::result::Result<(u64, u64), String> {
    let bad = || format!("size '{s}' is not WIDTHxHEIGHT in pixels (for example 1024x1024)");
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let num = |t: &str| {
        (!t.is_empty() && t.len() <= 5 && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse::<u64>().ok())
            .flatten()
    };
    let (w, h) = (num(w).ok_or_else(bad)?, num(h).ok_or_else(bad)?);
    if w == 0 || h == 0 || w % 16 != 0 || h % 16 != 0 {
        return Err(format!(
            "size {s}: width and height must both be multiples of 16"
        ));
    }
    let px = w * h;
    if !(655_360..=8_294_400).contains(&px) {
        return Err(format!(
            "size {s}: an image must have between 655,360 and 8,294,400 pixels"
        ));
    }
    if w.max(h) > 3840 {
        return Err(format!("size {s}: no side may be longer than 3840 pixels"));
    }
    if w.max(h) > 3 * w.min(h) {
        return Err(format!(
            "size {s}: the long side may be at most three times the short"
        ));
    }
    Ok((w, h))
}

/// The output tokens of one image by OpenAI's rule (the image generation guide's calculator, read
/// 30 Oct 2026): the quality's base figure scaled down along the short side, rounded half to even,
/// times the pixel term.
pub fn output_tokens(base: u64, w: u64, h: u64) -> u64 {
    let (long, short) = (w.max(h), w.min(h));
    let (num, den) = (base * short, long);
    let (fl, rem) = (num / den, num % den);
    let scaled = match (rem * 2).cmp(&den) {
        std::cmp::Ordering::Less => fl,
        std::cmp::Ordering::Greater => fl + 1,
        std::cmp::Ordering::Equal => fl + fl % 2,
    };
    (base * scaled * (2_000_000 + w * h)).div_ceil(4_000_000)
}

/// One image request made ready for the guard: its route, the body to send (the route's model in
/// `model`) and the bounds it is reserved on.
#[derive(Debug, Clone)]
pub struct Plan {
    pub kind: Kind,
    pub label: String,
    pub route: Route,
    pub body: Value,
    pub n: u64,
    pub size: (u64, u64),
    pub quality: String,
    pub reference_images: usize,
    pub input_bound: u64,
    pub max_output: u64,
}

/// A request refused or failed, with the HTTP status a service front answers and the words.
#[derive(Debug, Clone)]
pub struct Failure {
    pub status: u16,
    pub error: String,
    pub detail: Value,
    /// Set when the request had been admitted: how it was settled (boxed, so a failure stays small).
    pub receipt: Option<Box<Receipt>>,
}

fn fail(status: u16, error: impl Into<String>) -> Failure {
    Failure {
        status,
        error: error.into(),
        detail: Value::Null,
        receipt: None,
    }
}

/// What an admitted image request cost.
#[derive(Debug, Clone)]
pub struct Receipt {
    pub id: String,
    pub route: String,
    pub model: String,
    pub reservation: u64,
    pub charge: Option<u64>,
    pub currency: String,
    pub basis: String,
    pub attempt: u64,
    pub retry_of: Option<String>,
}

impl Receipt {
    pub fn to_json(&self) -> Value {
        json!({"id":self.id,"route":self.route,"model":self.model,"reservation":fmt(self.reservation),"charge":{"amount":self.charge.map(fmt).unwrap_or_else(|| "unknown".into()),"currency":self.currency,"basis":self.basis},"attempt":self.attempt,"retry_of":self.retry_of})
    }
}

/// The image route a request names in `model`: none (the default route), an `images:` route label,
/// or the default route's own model name.
fn route_for(layer: &Layer, asked: Option<&str>) -> std::result::Result<(String, Route), Failure> {
    let label = match asked {
        None => DEFAULT_ROUTE.to_owned(),
        Some(l) if l.starts_with("images:") => l.to_owned(),
        Some(m) => match layer.route(DEFAULT_ROUTE) {
            Ok(r) if r.model == m => DEFAULT_ROUTE.to_owned(),
            _ => {
                return Err(fail(
                    400,
                    format!(
                        "'{m}' is not an image route; name one such as {DEFAULT_ROUTE}, or leave model out"
                    ),
                ));
            }
        },
    };
    let route = layer.route(&label).map_err(|e| fail(400, e.to_string()))?;
    if route.provider != "openai" {
        return Err(fail(
            400,
            format!("the image route {label} names a provider that offers no images"),
        ));
    }
    Ok((label, route))
}

/// Check an image request and work out the bounds it is reserved on; refused unsent in plain words.
pub fn plan(layer: &Layer, kind: Kind, body: &Value) -> std::result::Result<Plan, Failure> {
    let Some(obj) = body.as_object() else {
        return Err(fail(400, "the body is not a JSON object"));
    };
    for k in obj.keys() {
        let allowed = GENERATE_FIELDS.contains(&k.as_str())
            || (kind == Kind::Edit && EDIT_FIELDS.contains(&k.as_str()));
        if !allowed {
            let why = match k.as_str() {
                "stream" | "partial_images" => " (streamed and partial images are not offered)",
                _ => "",
            };
            return Err(fail(
                400,
                format!("the field '{k}' is not offered on the image route{why}"),
            ));
        }
    }
    let (label, route) = route_for(layer, body["model"].as_str())?;
    let prices = prices::load(&layer.prices_path())
        .map_err(|e| fail(503, format!("unknown pricing: {e}")))?;
    let Some(image) = prices
        .find(&route.provider, &route.model)
        .and_then(|p| p.image.clone())
    else {
        return Err(fail(
            503,
            format!(
                "unknown pricing: no image price for {}:{}",
                route.provider, route.model
            ),
        ));
    };
    let prompt = body["prompt"].as_str().unwrap_or("");
    if prompt.trim().is_empty() {
        return Err(fail(400, "the request needs a prompt"));
    }
    if prompt.chars().count() > 32_000 {
        return Err(fail(400, "the prompt is longer than 32,000 characters"));
    }
    let n = match body.get("n") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=MAX_N).contains(n))
            .ok_or_else(|| fail(400, format!("n must be a whole number from 1 to {MAX_N}")))?,
    };
    let size_text = body["size"].as_str().unwrap_or("");
    if size_text.is_empty() || size_text == "auto" {
        return Err(fail(
            400,
            "name the size (for example 1024x1024): an automatic size cannot be priced before sending",
        ));
    }
    let size = parse_size(size_text).map_err(|e| fail(400, e))?;
    let quality = body["quality"].as_str().unwrap_or("");
    if quality.is_empty() || quality == "auto" {
        return Err(fail(
            400,
            "name the quality (for example low): an automatic quality cannot be priced before sending",
        ));
    }
    let Some(base) = image.base(quality) else {
        let known: Vec<&str> = image.output_base.iter().map(|(q, _)| q.as_str()).collect();
        return Err(fail(
            400,
            format!("quality '{quality}' is not one of {}", known.join(", ")),
        ));
    };
    let mut reference_images = 0;
    if kind == Kind::Edit {
        let list = body["images"]
            .as_array()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| fail(400, "an edit needs at least one reference image in images"))?;
        if list.len() > MAX_IMAGES {
            return Err(fail(
                400,
                format!("an edit takes at most {MAX_IMAGES} reference images"),
            ));
        }
        let one = |v: &Value| {
            let url = v["image_url"].as_str();
            let file = v["file_id"].as_str();
            let ok = v.as_object().is_some_and(|o| o.len() == 1)
                && (url.is_some_and(|u| u.starts_with("data:image/") || u.starts_with("https://"))
                    || file.is_some_and(|f| !f.is_empty()));
            ok.then_some(())
        };
        for v in list {
            one(v).ok_or_else(|| {
                fail(
                    400,
                    "each reference image is {\"image_url\": a data URL or https address} or {\"file_id\": an id}",
                )
            })?;
        }
        reference_images = list.len();
        // A mask or markup image is read as input too.
        for k in ["mask", "markup"] {
            if let Some(v) = body.get(k).filter(|v| !v.is_null()) {
                one(v).ok_or_else(|| fail(400, format!("{k} must be one image, as in images")))?;
                reference_images += 1;
            }
        }
    }
    let per_image = output_tokens(base, size.0, size.1) + OUTPUT_SLACK;
    let max_output = n * per_image;
    if max_output > route.max_output {
        return Err(fail(
            400,
            format!(
                "this call could make up to {max_output} image tokens; the route {label} allows at most {} in one call",
                route.max_output
            ),
        ));
    }
    let input_bound =
        money::FRAME + prompt.len() as u64 + reference_images as u64 * image.input_tokens_per_image;
    let mut send = body.clone();
    send["model"] = json!(route.model);
    Ok(Plan {
        kind,
        label,
        route,
        body: send,
        n,
        size,
        quality: quality.to_owned(),
        reference_images,
        input_bound,
        max_output,
    })
}

/// A fixed failure classification; arbitrary provider text is never a display diagnostic.
fn provider_message(body: &[u8], key: &keys::Secret) -> Value {
    keys::provider_error(body, key)["error"]["message"]
        .as_str()
        .map(|m| json!(m))
        .unwrap_or(Value::Null)
}

/// Admit, send and settle one image request for `tool`. The reply is the provider's (`data[]` with
/// `b64_json`, `usage`); the receipt says what was reserved and charged.
pub fn run(
    layer: &Layer,
    tool: &str,
    kind: Kind,
    body: &Value,
    timeout_ms: u64,
    cancelled: &dyn Fn() -> bool,
) -> std::result::Result<(Value, Receipt), Failure> {
    let plan = plan(layer, kind, body)?;
    let route = &plan.route;
    let Some(var) = keys::var_for(&route.provider) else {
        return Err(fail(503, "the image route's provider has no key"));
    };
    if !layer.has_key(var) {
        return Err(fail(
            503,
            format!(
                "This route needs an {} key in the shared key store, and none is there",
                provider_name(&route.provider)
            ),
        ));
    }
    if let Err(e) = chat::check_base(&route.provider, &route.base) {
        return Err(fail(503, e.to_string()));
    }
    let fp = money::fingerprint(&plan.label, &plan.body, plan.max_output);
    let ask = Ask {
        tool,
        id: None,
        route: &plan.label,
        provider: &route.provider,
        model: &route.model,
        input_bound: plan.input_bound,
        max_output: plan.max_output,
        fingerprint: &fp,
        inner: None,
    };
    let admitted = layer.admit(&ask).map_err(|r| Failure {
        status: r.status(),
        error: r.reason.clone(),
        detail: r.figures.clone(),
        receipt: None,
    })?;
    let keep_dir = layer.home.join("failed");
    let receipt = |settled: money::Settled| Receipt {
        id: admitted.id.clone(),
        route: plan.label.clone(),
        model: route.model.clone(),
        reservation: admitted.nano,
        charge: settled.charge,
        currency: admitted.currency.clone(),
        basis: settled.basis,
        attempt: admitted.attempt,
        retry_of: admitted.retry_of.clone(),
    };
    let key = match layer.key(var) {
        Ok(k) => k,
        Err(_) => {
            let s = layer.settle(
                &admitted,
                Billing::NotSent("key not in the key store".into()),
            );
            return Err(Failure {
                receipt: Some(Box::new(receipt(s))),
                ..fail(503, "the key store could not be read; nothing was sent")
            });
        }
    };
    let url = format!(
        "{}/{}",
        route.base.trim_end_matches('/'),
        plan.kind.endpoint()
    );
    let ran = chat::post_key(
        &key,
        &route.provider,
        &url,
        &plan.body,
        timeout_ms,
        cancelled,
        &mut |_| true,
        REPLY_LIMIT,
    );
    let settle_fail = |billing: Billing, status: u16, error: String, detail: Value| {
        let s = layer.settle(&admitted, billing);
        Err(Failure {
            status,
            error,
            detail,
            receipt: Some(Box::new(receipt(s))),
        })
    };
    if !ran.reason.is_empty() {
        if !ran.body.is_empty() {
            chat::keep(
                &keep_dir,
                &admitted.id,
                ran.status.unwrap_or(502),
                &ran.body,
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
            (_, Some(28)) | ("timeout", _) => {
                Billing::Unknown(format!("provider timed out (curl exit 28){}", ran.said()))
            }
            (reason, exit) => {
                Billing::Unknown(format!("{reason} (curl exit {exit:?}){}", ran.said()))
            }
        };
        let status = if ran.reason == "timeout" { 504 } else { 502 };
        return settle_fail(
            billing,
            status,
            "the provider call failed".into(),
            Value::Null,
        );
    }
    let Some(status) = ran.status else {
        return settle_fail(
            Billing::Unknown("no HTTP status".into()),
            502,
            "the provider call failed".into(),
            Value::Null,
        );
    };
    if status != 200 {
        chat::keep(&keep_dir, &admitted.id, status, &ran.body, Some(&key));
        let billing = if money::not_billed(status) {
            Billing::Rejected(status)
        } else {
            Billing::Unknown(format!("provider HTTP {status}"))
        };
        let said = provider_message(&ran.body, &key);
        return settle_fail(
            billing,
            502,
            format!("the provider refused the request (HTTP {status})"),
            json!({"provider_status":status,"provider_message":said}),
        );
    }
    let Ok(reply) = serde_json::from_slice::<Value>(&ran.body) else {
        chat::keep(&keep_dir, &admitted.id, status, &ran.body, Some(&key));
        return settle_fail(
            Billing::Unknown("reply is not JSON".into()),
            502,
            "the provider's reply could not be read".into(),
            Value::Null,
        );
    };
    let billing = if reply["usage"].is_object() {
        // Settled on the usage alone; the images stay out of the ledger.
        let served = reply["model"].as_str().unwrap_or(&route.model).to_owned();
        let generation = reply["id"].as_str().map(str::to_owned).or_else(|| {
            reply["created"]
                .as_u64()
                .map(|c| format!("image-created-{c}"))
        });
        Billing::Usage {
            reply: json!({"id":generation,"model":served,"usage":reply["usage"]}),
            served,
        }
    } else {
        Billing::Unknown("reply carried no usage".into())
    };
    let whole = reply["data"]
        .as_array()
        .is_some_and(|d| !d.is_empty() && d.iter().all(|i| i["b64_json"].is_string()));
    let settled = layer.settle(&admitted, billing);
    let receipt = receipt(settled);
    if !whole {
        return Err(Failure {
            status: 502,
            error: "the provider's reply held no images".into(),
            detail: Value::Null,
            receipt: Some(Box::new(receipt)),
        });
    }
    Ok((reply, receipt))
}

/// The service front: `POST /tool/<tool>/v1/images/generations` or `.../v1/images/edits`, OpenAI's
/// request form (JSON; reference images as `images: [{"image_url": "data:image/png;base64,..."}]`).
/// Replied whole: the provider's reply with the layer's receipt (`id`, `route`, `charge`).
pub fn handle(
    layer: &Layer,
    tool: &str,
    kind: Kind,
    body: &[u8],
    s: &mut TcpStream,
    timeout_ms: u64,
) {
    let Ok(body) = serde_json::from_slice::<Value>(body) else {
        return respond(s, 400, &json!({"error":"the body is not JSON"}));
    };
    let client = s.try_clone().ok();
    match run(layer, tool, kind, &body, timeout_ms, &|| {
        client.as_ref().is_some_and(disconnected)
    }) {
        Ok((mut reply, r)) => {
            let receipt = r.to_json();
            for k in [
                "id",
                "route",
                "charge",
                "attempt",
                "retry_of",
                "reservation",
            ] {
                reply[k] = receipt[k].clone();
            }
            respond(s, 200, &reply);
        }
        Err(f) => {
            let mut v = json!({"error":f.error,"detail":f.detail});
            if let Some(r) = f.receipt {
                let r = r.to_json();
                v["id"] = r["id"].clone();
                v["charge"] = r["charge"].clone();
            }
            respond(s, f.status, &v);
        }
    }
}

// ---- base64, for reference images read from files and images written to files ----

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace()) {
        if b == b'=' {
            break;
        }
        let v = B64.iter().position(|c| *c == b)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

/// A reference image file as a data URL, its type read from its first bytes (PNG, JPEG or WebP).
pub fn data_url(path: &Path) -> std::result::Result<String, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mime = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "image/webp"
    } else {
        return Err(format!("{}: not a PNG, JPEG or WebP image", path.display()));
    };
    Ok(format!("data:{mime};base64,{}", b64_encode(&b)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_error_fields_are_masked_before_parsing_and_display() {
        let key = keys::Secret::local("sk-fixture-PREFIX!TAIL-SENTINEL".into());
        let raw = json!({"error":{"message":key.expose()}}).to_string();
        assert_eq!(
            provider_message(raw.as_bytes(), &key),
            json!("the provider call failed")
        );
        assert_eq!(
            provider_message(b"unreadable sk-fixture-PREFIX!TAIL-SENTINEL", &key),
            json!("the provider call failed")
        );
    }
    #[test]
    fn output_tokens_follow_openais_rule() {
        // GPT Image 2 (low 16, medium 48, high 96) at 30 USD per million tokens reproduces the guide's
        // per-image table: 0.006, 0.053, 0.211 square; 0.005, 0.041, 0.165 portrait.
        assert_eq!(output_tokens(16, 1024, 1024), 196);
        assert_eq!(output_tokens(48, 1024, 1024), 1756);
        assert_eq!(output_tokens(96, 1024, 1024), 7024);
        assert_eq!(output_tokens(16, 1024, 1536), 158);
        assert_eq!(output_tokens(96, 1536, 1024), 5488);
        // GPT Image 2.5 low at the smallest allowed size.
        assert_eq!(output_tokens(16, 1024, 640), 107);
        assert_eq!(output_tokens(96, 3840, 2160), 13_342);
    }
    #[test]
    fn sizes_are_checked() {
        assert_eq!(parse_size("1024x640"), Ok((1024, 640)));
        assert_eq!(parse_size("3840x2160"), Ok((3840, 2160)));
        for bad in [
            "1024",
            "1000x1000",
            "512x512",
            "4096x2160",
            "2400x768",
            "x",
            "auto",
        ] {
            assert!(parse_size(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn base64_round_trips() {
        for s in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"\x89PNG\r\n\x1a\n\0\xff",
        ] {
            assert_eq!(b64_decode(&b64_encode(s)).as_deref(), Some(s));
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert!(b64_decode("@@").is_none());
    }
}
