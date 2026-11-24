//! The one dated price table (`prices.json`, schema qroc-prices/1, lifted from QROC's route-shim
//! `money.rs`): exact decimals, prices per million tokens kept as nano-units per token, the
//! reservation rule and the charge of a reply from its usage. Read again on every request.
use crate::{Error, Result, io_at};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub const PRICES_SCHEMA: &str = "qroc-prices/1";

/// Parse a non-negative decimal (string or JSON number) into units of 10^-scale, exactly.
fn decimal(v: &Value, scale: u32) -> Option<u64> {
    let s = match v {
        Value::String(s) => s.trim().to_owned(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    if int.is_empty()
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let frac = frac.trim_end_matches('0');
    if frac.len() > scale as usize {
        return None;
    }
    let whole: u64 = int.parse().ok()?;
    let part: u64 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<width$}", width = scale as usize)
            .parse()
            .ok()?
    };
    whole.checked_mul(10u64.pow(scale))?.checked_add(part)
}

/// A price "per million tokens" as nano-units per token (2.50/MTok = 2500 nano per token).
pub fn per_token(v: &Value) -> Option<u64> {
    decimal(v, 3)
}

/// An amount of currency as nano-units.
pub fn amount(v: &Value) -> Option<u64> {
    decimal(v, 9)
}

pub fn fmt(nano: u64) -> String {
    format!("{}.{:09}", nano / 1_000_000_000, nano % 1_000_000_000)
}

/// An amount for people: dollars and cents, with the exact figure kept when a cent rounds it away.
pub fn plain(nano: u64) -> String {
    let cents = nano / 10_000_000;
    let rest = nano % 10_000_000;
    if rest == 0 {
        format!("{}.{:02}", cents / 100, cents % 100)
    } else {
        let s = fmt(nano);
        s.trim_end_matches('0').to_owned()
    }
}

/// Consumers' tests build it field by field: `Price { .., ..Default::default() }` keeps them
/// compiling when a field is added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Price {
    pub key: String,
    pub model: String,
    pub input: u64,
    pub cached_input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub output: u64,
    /// (input tokens above which the long prices apply, input, cached input, output)
    pub long: Option<(u64, u64, u64, u64)>,
    /// An image model's own prices and bounds; `input` is then its text input and `output` its image
    /// output.
    pub image: Option<ImagePrice>,
}

/// The prices and bounds of an image model (`"kind":"image"` in the table): image input per token,
/// the official output token rule's base figure per quality, and the most input tokens one reference
/// image is reserved at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImagePrice {
    pub image_input: u64,
    pub cached_image_input: u64,
    /// Quality name → the base of OpenAI's output token rule (low 16 ... max 96 for GPT Image 2.5).
    pub output_base: Vec<(String, u64)>,
    pub input_tokens_per_image: u64,
}

impl ImagePrice {
    pub fn base(&self, quality: &str) -> Option<u64> {
        self.output_base
            .iter()
            .find(|(q, _)| q == quality)
            .map(|(_, b)| *b)
    }
}

#[derive(Debug, Clone)]
pub struct Prices {
    pub currency: String,
    pub sha256: Option<String>,
    pub models: Vec<Price>,
}

/// The price table, or why it cannot be used (the message is the guard's refusal text).
pub fn load(path: &Path) -> std::result::Result<Prices, String> {
    let bytes = fs::read(path).map_err(|e| format!("price file unreadable: {e}"))?;
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|_| "price file is not JSON".to_string())?;
    if v["schema"] != PRICES_SCHEMA {
        return Err("price file schema is not qroc-prices/1".into());
    }
    let currency = v["currency"]
        .as_str()
        .ok_or("price file has no currency")?
        .to_owned();
    let obj = v["models"].as_object().ok_or("price file has no models")?;
    let mut models = Vec::new();
    for (key, m) in obj {
        let get = |k: &str| per_token(&m[k]).ok_or(format!("price {key}.{k} missing or not exact"));
        let input = get("input")?;
        let opt = |k: &str| match m.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(x) => per_token(x)
                .map(Some)
                .ok_or(format!("price {key}.{k} not exact")),
        };
        let long = match m
            .get("long_context_above_input_tokens")
            .and_then(Value::as_u64)
        {
            None => None,
            Some(t) => Some((
                t,
                get("long_input")?,
                get("long_cached_input")?,
                get("long_output")?,
            )),
        };
        let image = if m["kind"] == "image" {
            let base = m["image_output_base"]
                .as_object()
                .filter(|o| !o.is_empty())
                .ok_or(format!("price {key}.image_output_base missing"))?;
            let mut output_base = Vec::new();
            for (q, b) in base {
                let b = b.as_u64().filter(|b| *b > 0).ok_or(format!(
                    "price {key}.image_output_base.{q} not a positive whole number"
                ))?;
                output_base.push((q.clone(), b));
            }
            Some(ImagePrice {
                image_input: get("image_input")?,
                cached_image_input: get("cached_image_input")?,
                output_base,
                input_tokens_per_image: m["input_tokens_per_image_bound"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or(format!("price {key}.input_tokens_per_image_bound missing"))?,
            })
        } else {
            None
        };
        if m["source_url"].as_str().is_none() || m["read_on"].as_str().is_none() {
            return Err(format!("price {key} lacks source_url or read_on"));
        }
        models.push(Price {
            key: key.clone(),
            model: m["model"]
                .as_str()
                .ok_or(format!("price {key}.model missing"))?
                .to_owned(),
            input,
            cached_input: get("cached_input")?,
            cache_write_5m: opt("cache_write_5m")?.unwrap_or(input),
            cache_write_1h: opt("cache_write_1h")?.unwrap_or(input),
            output: get("output")?,
            long,
            image,
        });
    }
    Ok(Prices {
        currency,
        sha256: sha256(&bytes),
        models,
    })
}

/// The price table as a library error, for fronts that check it at start.
pub fn load_checked(path: &Path) -> Result<Prices> {
    load(path).map_err(Error::Invalid)
}

impl Prices {
    /// The price of `provider:model`; a served model also matches its dated snapshot (`model-...`).
    pub fn find(&self, provider: &str, model: &str) -> Option<&Price> {
        self.models.iter().find(|p| {
            p.key.split_once(':').is_some_and(|(pr, _)| pr == provider)
                && (model == p.model
                    || model
                        .strip_prefix(p.model.as_str())
                        .is_some_and(|r| r.starts_with('-')))
        })
    }
}

/// The reservation: input bound at the dearest input rate of its tier (cache writes included; an
/// image model's image input rate) plus the output cap at the output rate of that tier.
pub fn reserve(p: &Price, input_tokens: u64, max_output: u64) -> Option<u64> {
    if let Some(i) = &p.image {
        return input_tokens
            .checked_mul(p.input.max(i.image_input))?
            .checked_add(max_output.checked_mul(p.output)?);
    }
    let (inp, out) = match p.long {
        Some((t, li, _, lo)) if input_tokens > t => {
            (li.max(p.cache_write_5m).max(p.cache_write_1h), lo)
        }
        _ => (
            p.input.max(p.cache_write_5m).max(p.cache_write_1h),
            p.output,
        ),
    };
    input_tokens
        .checked_mul(inp)?
        .checked_add(max_output.checked_mul(out)?)
}

/// The charge of a reply from its usage, or why it is unknown. `reply` is the provider's reply in its
/// own form: OpenAI chat completions (`prompt_tokens`...), OpenAI Responses (`input_tokens` with
/// `input_tokens_details`), or Anthropic Messages.
pub fn charge(provider: &str, p: &Price, reply: &Value) -> std::result::Result<u64, String> {
    let u = &reply["usage"];
    let n = |v: &Value| v.as_u64();
    if let Some(i) = &p.image {
        // The Image API's usage: input split into text and image tokens, output tokens all priced at
        // the image output rate (the table lists no other output rate for these models). Cached input
        // is not reported by this API, so none is assumed.
        if provider != "openai" {
            return Err(format!(
                "image models of provider '{provider}' are not priced"
            ));
        }
        let input = n(&u["input_tokens"]).ok_or("usage lacks input_tokens")?;
        let output = n(&u["output_tokens"]).ok_or("usage lacks output_tokens")?;
        let d = &u["input_tokens_details"];
        let (text, image) = match (n(&d["text_tokens"]), n(&d["image_tokens"])) {
            (Some(t), Some(im)) if t + im == input => (t, im),
            // Without a split that adds up, all input is charged at the dearer image rate.
            _ => (0, input),
        };
        return Ok(text * p.input + image * i.image_input + output * p.output);
    }
    match provider {
        // OpenRouter speaks OpenAI's chat completions and Responses forms, usage included.
        "openai" | "openrouter" => {
            if let Some(t) = reply["service_tier"].as_str()
                && !matches!(t, "default" | "standard")
            {
                return Err(format!("service tier '{t}' is not priced"));
            }
            // Chat completions name them prompt/completion; the Responses API input/output.
            let prompt = n(&u["prompt_tokens"])
                .or_else(|| n(&u["input_tokens"]))
                .ok_or("usage lacks prompt_tokens")?;
            let completion = n(&u["completion_tokens"])
                .or_else(|| n(&u["output_tokens"]))
                .ok_or("usage lacks completion_tokens")?;
            let cached = n(&u["prompt_tokens_details"]["cached_tokens"])
                .or_else(|| n(&u["input_tokens_details"]["cached_tokens"]))
                .unwrap_or(0)
                .min(prompt);
            let (i, c, o) = match p.long {
                Some((t, li, lc, lo)) if prompt > t => (li, lc, lo),
                _ => (p.input, p.cached_input, p.output),
            };
            Ok((prompt - cached) * i + cached * c + completion * o)
        }
        "anthropic" => {
            if let Some(t) = u["service_tier"].as_str()
                && t != "standard"
            {
                return Err(format!("service tier '{t}' is not priced"));
            }
            if let Some(g) = u["inference_geo"].as_str()
                && !matches!(g, "global" | "not_available")
            {
                return Err(format!("inference geography '{g}' is not priced"));
            }
            if let Some(s) = u["speed"].as_str()
                && s != "standard"
            {
                return Err(format!("speed '{s}' is not priced"));
            }
            let input = n(&u["input_tokens"]).ok_or("usage lacks input_tokens")?;
            let output = n(&u["output_tokens"]).ok_or("usage lacks output_tokens")?;
            let read = n(&u["cache_read_input_tokens"]).unwrap_or(0);
            let write = n(&u["cache_creation_input_tokens"]).unwrap_or(0);
            let write_1h = n(&u["cache_creation"]["ephemeral_1h_input_tokens"])
                .unwrap_or(0)
                .min(write);
            Ok(input * p.input
                + read * p.cached_input
                + (write - write_1h) * p.cache_write_5m
                + write_1h * p.cache_write_1h
                + output * p.output)
        }
        _ => Err(format!("provider '{provider}' is not priced")),
    }
}

pub fn sha256(bytes: &[u8]) -> Option<String> {
    let mut c = Command::new("/usr/bin/sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    c.stdin.take()?.write_all(bytes).ok()?;
    let o = c.wait_with_output().ok()?;
    String::from_utf8(o.stdout)
        .ok()?
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

/// Read a JSON settings file.
pub(crate) fn read_json(path: &Path) -> Result<Value> {
    let b = fs::read(path).map_err(io_at(path))?;
    serde_json::from_slice(&b).map_err(|_| Error::Json {
        path: path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn decimals_are_exact() {
        assert_eq!(per_token(&json!("2.50")), Some(2500));
        assert_eq!(per_token(&json!("0.10")), Some(100));
        assert_eq!(per_token(&json!("4")), Some(4000));
        assert_eq!(
            per_token(&json!("0.0001")),
            None,
            "finer than nano per token is refused"
        );
        assert_eq!(amount(&json!("1.000000001")), Some(1_000_000_001));
        assert_eq!(amount(&json!(-1)), None);
        assert_eq!(fmt(1_000_000_001), "1.000000001");
        assert_eq!(plain(1_230_000_000), "1.23");
        assert_eq!(plain(62_400), "0.0000624");
    }
    #[test]
    fn openai_charge_splits_cached_and_long_context() {
        let p = Price {
            key: "openai:m".into(),
            model: "m".into(),
            input: 2000,
            cached_input: 100,
            cache_write_5m: 2000,
            cache_write_1h: 2000,
            output: 10000,
            long: Some((272000, 4000, 200, 15000)),
            image: None,
        };
        let r = json!({"usage":{"prompt_tokens":1000,"completion_tokens":10,"prompt_tokens_details":{"cached_tokens":400}}});
        assert_eq!(
            charge("openai", &p, &r),
            Ok(600 * 2000 + 400 * 100 + 10 * 10000)
        );
        let r = json!({"usage":{"input_tokens":1000,"output_tokens":10,"input_tokens_details":{"cached_tokens":400}}});
        assert_eq!(
            charge("openai", &p, &r),
            Ok(600 * 2000 + 400 * 100 + 10 * 10000),
            "Responses usage"
        );
        let r = json!({"usage":{"prompt_tokens":300000,"completion_tokens":1}});
        assert_eq!(charge("openai", &p, &r), Ok(300000 * 4000 + 15000));
        assert!(charge("openai", &p, &json!({"usage":{}})).is_err());
        assert!(
            charge(
                "openai",
                &p,
                &json!({"service_tier":"flex","usage":{"prompt_tokens":1,"completion_tokens":1}})
            )
            .is_err()
        );
        assert_eq!(reserve(&p, 300001, 10), Some(300001 * 4000 + 10 * 15000));
    }
    #[test]
    fn image_charge_splits_text_and_image_input() {
        let p = Price {
            key: "openai:img".into(),
            model: "img".into(),
            input: 5000,
            cached_input: 1250,
            cache_write_5m: 5000,
            cache_write_1h: 5000,
            output: 30000,
            long: None,
            image: Some(ImagePrice {
                image_input: 8000,
                cached_image_input: 2000,
                output_base: vec![("low".into(), 16)],
                input_tokens_per_image: 1000,
            }),
        };
        let r = json!({"usage":{"input_tokens":120,"input_tokens_details":{"text_tokens":20,"image_tokens":100},"output_tokens":196,"total_tokens":316}});
        assert_eq!(
            charge("openai", &p, &r),
            Ok(20 * 5000 + 100 * 8000 + 196 * 30000)
        );
        let r = json!({"usage":{"input_tokens":120,"output_tokens":196}});
        assert_eq!(
            charge("openai", &p, &r),
            Ok(120 * 8000 + 196 * 30000),
            "no split: the dearer rate"
        );
        assert!(charge("openai", &p, &json!({"data":[]})).is_err());
        assert_eq!(reserve(&p, 10, 196), Some(10 * 8000 + 196 * 30000));
    }
}
