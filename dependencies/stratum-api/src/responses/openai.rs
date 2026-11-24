//! OpenAI's Responses API, passed through: the request gets the route's model and output cap and nothing
//! of Zeno's that would reach another service; the reply's events go back to Zeno unchanged while the
//! layer watches them for the final usage, which settles the charge.
use super::sse::Event;
use serde_json::{Value, json};

/// The body sent to OpenAI: Zeno's request with the route's model, `max_output_tokens` set to the
/// route's cap (so the reservation bounds the charge), streaming on, nothing stored at the provider.
pub fn request(body: &Value, model: &str, max_output: u64, effort: Option<&str>) -> Value {
    let mut b = body.clone();
    b["model"] = json!(model);
    b["max_output_tokens"] = json!(max_output);
    b["stream"] = json!(true);
    b["store"] = json!(false);
    if let Some(e) = effort {
        if !b["reasoning"].is_object() {
            b["reasoning"] = json!({});
        }
        b["reasoning"]["effort"] = json!(e);
    }
    // Zeno's own bookkeeping fields for its first-party backend; the public API does not take them.
    if let Some(o) = b.as_object_mut() {
        o.remove("client_metadata");
        o.remove("access_programs");
        // Only the standard tier is priced; a faster tier would leave the charge unknown.
        o.remove("service_tier");
    }
    b
}

/// What the pass-through has seen of the reply: the final response (from `response.completed`, or a
/// `response.incomplete` that still carries usage), and any failure event.
#[derive(Debug, Default)]
pub struct Watch {
    pub completed: Option<Value>,
    pub failed: Option<String>,
}

/// What settlement keeps of a final response: its id, model, usage and the tier it was served at. The tier
/// is a short provider name the price check refuses unless it is the standard one; anything that is not a
/// plain short lower-case word is kept as `unrecognised`, so it is refused too and no free text is kept.
fn snapshot(r: &Value) -> Value {
    let mut s = json!({"id":r["id"],"model":r["model"],"usage":r["usage"]});
    match &r["service_tier"] {
        Value::Null => {}
        Value::String(t)
            if !t.is_empty()
                && t.len() <= 32
                && t.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') =>
        {
            s["service_tier"] = json!(t)
        }
        _ => s["service_tier"] = json!("unrecognised"),
    }
    s
}

impl Watch {
    pub fn see(&mut self, e: &Event) {
        let Ok(v) = serde_json::from_str::<Value>(&e.data) else {
            return;
        };
        let kind = v["type"].as_str().unwrap_or(e.name.as_str());
        match kind {
            "response.completed" | "response.incomplete" => {
                self.completed = Some(snapshot(&v["response"]))
            }
            "response.failed" => {
                self.failed = Some("provider reported failure".into());
                if v["response"]["usage"].is_object() {
                    self.completed = Some(snapshot(&v["response"]));
                }
            }
            "error" => {
                self.failed = Some("provider stream error".into());
            }
            _ => {}
        }
    }

    /// The final response object, when its usage arrived: the layer's settlement reads Responses usage
    /// (`input_tokens`, cached part, `output_tokens`) directly. None leaves the charge unknown.
    pub fn settlement(&self) -> Option<Value> {
        self.completed
            .clone()
            .filter(|r| r["usage"]["input_tokens"].is_u64() && r["usage"]["output_tokens"].is_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watch_failure_state_and_settlement_never_keep_the_provider_error_object() {
        for kind in ["error", "response.failed"] {
            let mut watch = Watch::default();
            watch.see(&Event{name:kind.into(),data:json!({"type":kind,"code":"TAIL-SENTINEL",
                "error":{"code":"TAIL-SENTINEL"},"response":{"id":"fixture","model":"fixture",
                "usage":{"input_tokens":1,"output_tokens":1},"error":{"code":"TAIL-SENTINEL","message":"TAIL-SENTINEL"}}}).to_string()});
            assert!(!format!("{:?}", watch.failed).contains("SENTINEL"));
            assert!(
                !watch
                    .settlement()
                    .unwrap_or(Value::Null)
                    .to_string()
                    .contains("SENTINEL")
            );
        }
    }
    #[test]
    fn settlement_keeps_the_tier_so_an_unpriced_tier_is_refused() {
        let p = crate::prices::Price {
            key: "openai:fixture".into(),
            model: "fixture".into(),
            input: 2000,
            cached_input: 100,
            cache_write_5m: 0,
            cache_write_1h: 0,
            output: 10000,
            long: None,
            image: None,
        };
        let usage = json!({"input_tokens":1000,"output_tokens":10,"input_tokens_details":{"cached_tokens":0}});
        for kind in [
            "response.completed",
            "response.incomplete",
            "response.failed",
        ] {
            for (tier, priced) in [
                (json!("default"), true),
                (json!("standard"), true),
                (Value::Null, true),
                (json!("priority"), false),
                (json!("flex"), false),
                (json!("scale"), false),
                (json!("sk-fixture-TAIL-SENTINEL"), false),
                (json!(7), false),
            ] {
                let mut response = json!({"id":"r","model":"fixture","usage":usage});
                if !tier.is_null() {
                    response["service_tier"] = tier.clone();
                }
                let mut w = Watch::default();
                w.see(&Event {
                    name: kind.into(),
                    data: json!({"type":kind,"response":response}).to_string(),
                });
                let s = w.settlement().unwrap();
                assert!(!s.to_string().contains("SENTINEL"));
                for provider in ["openai", "openrouter"] {
                    let charged = crate::prices::charge(provider, &p, &s);
                    assert_eq!(
                        charged.is_ok(),
                        priced,
                        "{kind} {tier} {provider}: {charged:?}"
                    );
                    if priced {
                        assert_eq!(charged, Ok(2_100_000));
                    }
                }
            }
        }
    }
    #[test]
    fn request_capped_and_cleaned() {
        let b = json!({"model":"x","input":[],"store":true,"stream":false,"client_metadata":{"a":"b"},"reasoning":{"effort":"high","summary":"auto"}});
        let r = request(&b, "gpt-6.1-sol", 4096, Some("medium"));
        assert_eq!(r["model"], "gpt-6.1-sol");
        assert_eq!(r["max_output_tokens"], 4096);
        assert_eq!(r["store"], false);
        assert_eq!(r["stream"], true);
        assert_eq!(r["reasoning"], json!({"effort":"medium","summary":"auto"}));
        assert!(r.get("client_metadata").is_none());
        assert!(
            request(&json!({"service_tier":"priority"}), "m", 1, None)
                .get("service_tier")
                .is_none()
        );
    }
    #[test]
    fn usage_from_completed() {
        let mut w = Watch::default();
        w.see(&Event { name: "response.completed".into(), data: json!({"type":"response.completed","response":{"id":"resp_1","model":"gpt-6.1-sol-2026-10-01","service_tier":"default","usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":40},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":12},"total_tokens":130}}}).to_string() });
        let s = w.settlement().unwrap();
        assert_eq!(s["usage"]["input_tokens"], 100);
        assert_eq!(s["model"], "gpt-6.1-sol-2026-10-01");
        assert!(Watch::default().settlement().is_none());
    }
}
