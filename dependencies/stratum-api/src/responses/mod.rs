//! The layer's Responses path: `POST /tool/<tool>/v1/responses`, the protocol Zeno speaks. The requested
//! model picks the route (`responses:openai`, `responses:anthropic` or `responses:openrouter` in the route
//! table); OpenAI and OpenRouter (which speaks the same protocol) are passed through with the guard, its events streamed back unchanged as they arrive; Anthropic is translated to
//! and from its Messages API. Every turn is one admitted call in the ledger under the tool in the path:
//! priced and reserved before it is sent (the route's output cap is set on the request, so the
//! reservation bounds the charge), settled from the final usage after. A cut stream, a failed reply or a
//! reply without usage stays reserved as unknown, as the guard's rule says.
pub mod anthropic;
pub mod openai;
pub mod sse;

use crate::{
    Ask, Billing, Layer,
    chat::{self, Route},
    keys, money,
    prices::fmt,
    serve::{disconnected, respond},
};
use serde_json::{Value, json};
use std::{io::Write, net::TcpStream};

/// The route labels of this path, one per provider.
pub const LABELS: [&str; 3] = [
    "responses:openai",
    "responses:anthropic",
    "responses:openrouter",
];

/// The Responses route whose model is the one asked for; a provider name also selects its route.
pub fn route_for(layer: &Layer, asked: &str) -> Result<Route, String> {
    let mut known = Vec::new();
    for label in LABELS {
        if let Ok(r) = layer.route(label) {
            if r.model == asked || r.provider == asked {
                return Ok(r);
            }
            known.push(r.model);
        }
    }
    Err(if known.is_empty() {
        "the layer has no Responses routes in its route table".into()
    } else {
        format!(
            "no Responses route serves the model '{asked}'; this layer serves {}",
            known.join(" and ")
        )
    })
}

/// An error in the form Zeno shows (`{"error":{"message","type","code"}}`).
fn error(message: &str, code: &str, detail: Value) -> Value {
    json!({"error":{"message":message,"type":"stratum_api","code":code,"detail":detail}})
}

fn head(s: &mut TcpStream, receipt: &str) -> std::io::Result<()> {
    s.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nx-stratum-api-receipt: {receipt}\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )
}

/// What the reply stream has done so far.
struct Relay<'a> {
    s: &'a mut TcpStream,
    receipt: String,
    started: bool,
    /// The provider's bytes held back while it is not yet known to be a stream (an error body).
    held: Vec<u8>,
    gone: bool,
}

impl Relay<'_> {
    /// Bytes for Zeno; the head is written on the first.
    fn send(&mut self, b: &[u8]) -> bool {
        if self.gone {
            return false;
        }
        if !self.started {
            self.started = true;
            if head(self.s, &self.receipt).is_err() {
                self.gone = true;
                return false;
            }
        }
        if self.s.write_all(b).and_then(|_| self.s.flush()).is_err() {
            self.gone = true;
        }
        !self.gone
    }
}

/// True once the reply's first bytes show an event stream (a refused request answers with one JSON body).
fn looks_like_stream(b: &[u8]) -> Option<bool> {
    let t = String::from_utf8_lossy(b);
    let t = t.trim_start();
    if t.is_empty() {
        None
    } else {
        Some(!t.starts_with('{') && !t.starts_with('<'))
    }
}

/// A provider as a person names it.
fn provider_name(p: &str) -> &str {
    match p {
        "openai" => "OpenAI",
        "anthropic" => "Anthropic",
        "openrouter" => "OpenRouter",
        other => other,
    }
}

/// What keeps a route from being sent, in plain words: no key stored, or a model with no dated price.
fn unready(layer: &Layer, route: &Route) -> Vec<String> {
    let name = provider_name(&route.provider);
    let mut out = Vec::new();
    if let Ok(prices) = crate::prices::load(&layer.prices_path())
        && prices.find(&route.provider, &route.model).is_none()
    {
        out.push(format!("the model {} is not on {name}'s price list in the shared API layer, so its cost cannot be checked", route.model));
    }
    if keys::var_for(&route.provider).is_some_and(|v| !layer.has_key(v)) {
        out.push(format!("no {name} key is stored in the shared API layer"));
    }
    out
}

/// Serve one Responses request of `tool`; writes the whole HTTP reply (head, streamed events) itself.
pub fn handle(layer: &Layer, tool: &str, body: &[u8], s: &mut TcpStream, timeout_ms: u64) {
    let Ok(req) = serde_json::from_slice::<Value>(body) else {
        return respond(
            s,
            400,
            &error("the body is not JSON", "invalid_request", Value::Null),
        );
    };
    if !req["input"].is_array() && !req["input"].is_string() {
        return respond(
            s,
            400,
            &error("the request has no input", "invalid_request", Value::Null),
        );
    }
    let asked = req["model"].as_str().unwrap_or("");
    let route = match route_for(layer, asked) {
        Ok(r) => r,
        Err(e) => return respond(s, 400, &error(&e, "no_route", Value::Null)),
    };
    let max_out = req["max_output_tokens"]
        .as_u64()
        .map_or(route.max_output, |n| n.min(route.max_output));
    let effort = route.effort.as_deref();
    let anthropic = route.provider == "anthropic";
    let (sent, tools) = if anthropic {
        let (b, t) = anthropic::request(&req, &route.model, max_out, effort);
        (b, Some(t))
    } else {
        (openai::request(&req, &route.model, max_out, effort), None)
    };
    let sent_bytes = sent.to_string();
    let fp = money::fingerprint(&route.label, &req["input"], max_out);
    let ask = Ask {
        tool,
        id: None,
        route: &route.label,
        provider: &route.provider,
        model: &route.model,
        input_bound: money::input_bound_bytes(sent_bytes.as_bytes()),
        max_output: max_out,
        fingerprint: &fp,
        inner: None,
    };
    // Two refusals said in plain words before the guard is asked: no key stored for the provider, and a model
    // that is not on the provider's price list (the guard would refuse it as unknown pricing).
    let missing = unready(layer, &route);
    if !missing.is_empty() {
        return respond(
            s,
            503,
            &error(
                &format!("{}; nothing was sent", missing.join(", and ")),
                "not_ready",
                Value::Null,
            ),
        );
    }
    let admitted = match layer.admit(&ask) {
        Ok(a) => a,
        Err(r) => {
            let code = if r.status() == 429 {
                "spend_cap"
            } else {
                "not_priced"
            };
            return respond(
                s,
                r.status(),
                &error(
                    &format!(
                        "the shared API layer refused this turn, unsent: {}",
                        r.reason
                    ),
                    code,
                    r.figures,
                ),
            );
        }
    };
    let settle = |billing: Billing| layer.settle(&admitted, billing);
    let key = match keys::var_for(&route.provider).map(|v| layer.key(v)) {
        Some(Ok(k)) => k,
        _ => {
            settle(Billing::NotSent("key not in the key store".into()));
            return respond(
                s,
                503,
                &error(
                    "the shared API layer holds no key for this provider",
                    "no_key",
                    Value::Null,
                ),
            );
        }
    };
    let url = format!(
        "{}/v1/{}",
        route.base,
        if anthropic { "messages" } else { "responses" }
    );
    let client = s.try_clone().ok();
    let cancelled = || client.as_ref().is_some_and(disconnected);
    let mut relay = Relay {
        s,
        receipt: admitted.id.clone(),
        started: false,
        held: Vec::new(),
        gone: false,
    };
    let mut parser = sse::Parser::default();
    let mut watch = openai::Watch::default();
    let mut tr = tools.map(anthropic::Translator::new);
    let mut stream: Option<bool> = None;
    let ran = chat::post_key(
        &key,
        &route.provider,
        &url,
        &sent,
        timeout_ms,
        &cancelled,
        &mut |chunk| {
            if stream.is_none() {
                relay.held.extend_from_slice(chunk);
                stream = looks_like_stream(&relay.held);
                if stream != Some(true) {
                    return true;
                }
                let held = std::mem::take(&mut relay.held);
                return pass(&mut relay, &mut parser, &mut watch, tr.as_mut(), &held);
            }
            if stream == Some(false) {
                relay.held.extend_from_slice(chunk);
                return true;
            }
            pass(&mut relay, &mut parser, &mut watch, tr.as_mut(), chunk)
        },
        chat::BODY_LIMIT,
    );
    if let Some(e) = parser.finish() {
        match tr.as_mut() {
            Some(t) => {
                for (k, v) in t.feed(&e) {
                    relay.send(sse::frame(&k, &v).as_bytes());
                }
            }
            None => {
                watch.see(&e);
                relay.send(safe_frame(&e).as_bytes());
            }
        }
    }
    let keep = |status: u16| {
        chat::keep(
            &layer.home.join("failed"),
            &admitted.id,
            status,
            &ran.body,
            Some(&key),
        )
    };
    // The usage the provider reported, if it arrived: the charge is then exact, however the run ended.
    let usage = match tr.as_ref() {
        Some(t) => anthropic::settlement(t).map(chat::usage),
        None => watch.settlement().map(|r| {
            let served = r["model"].as_str().unwrap_or(&route.model).to_owned();
            Billing::Usage { reply: r, served }
        }),
    };
    let finished = match tr.as_ref() {
        Some(t) => t.finished && t.error.is_none(),
        None => watch.completed.is_some() && watch.failed.is_none(),
    };
    let billing = if !ran.reason.is_empty() {
        if !ran.body.is_empty() {
            keep(ran.status.unwrap_or(502));
        }
        match (ran.reason, ran.exit, usage) {
            ("spawn failed", _, _) => Billing::NotSent("curl did not start".into()),
            ("key service unreachable", _, _) => {
                Billing::NotSent("the key service could not be reached".into())
            }
            ("key service refused", _, _) => {
                Billing::NotSent("the key service refused the request; nothing was sent".into())
            }
            (_, Some(n @ (6 | 7)), _) => {
                Billing::NotSent(format!("curl could not connect (exit {n})"))
            }
            (_, _, Some(u)) => u,
            ("timeout", _, _) | (_, Some(28), _) => {
                Billing::Unknown(format!("provider timed out{}", ran.said()))
            }
            (reason, exit, None) => Billing::Unknown(format!(
                "stream cut before its final usage ({reason}, curl exit {exit:?}){}",
                ran.said()
            )),
        }
    } else {
        match ran.status {
            None => Billing::Unknown("no HTTP status".into()),
            Some(200) => match usage {
                Some(u) => u,
                None => {
                    keep(200);
                    let detail = watch
                        .failed
                        .clone()
                        .or_else(|| tr.as_ref().and_then(|t| t.error.clone()))
                        .unwrap_or_else(|| "stream ended without its final usage".into());
                    Billing::Unknown(
                        String::from_utf8_lossy(&keys::mask(detail.as_bytes(), &[&key]))
                            .into_owned(),
                    )
                }
            },
            Some(st) => {
                keep(st);
                if money::not_billed(st) {
                    Billing::Rejected(st)
                } else {
                    Billing::Unknown(format!("provider HTTP {st}"))
                }
            }
        }
    };
    let settled = settle(billing);
    let charge = settled.charge.map(fmt).unwrap_or_else(|| "unknown".into());
    if relay.started {
        if !finished && !relay.gone {
            // Zeno learns the turn failed instead of waiting on a stream that ends without its last event.
            let already = watch.failed.is_some() || tr.as_ref().is_some_and(|t| t.error.is_some());
            if !already {
                let (code, msg) = (
                    "stream_cut",
                    "the provider's stream ended before the turn was complete",
                );
                let events = match tr.as_mut() {
                    Some(t) => t.failed(code, msg),
                    None => vec![(
                        "response.failed".to_owned(),
                        json!({"type":"response.failed","response":{"id":admitted.id,"status":"failed","error":{"code":code,"message":msg}}}),
                    )],
                };
                for (k, v) in events {
                    relay.send(sse::frame(&k, &v).as_bytes());
                }
            }
        }
        // A comment line: event-stream readers skip it; it names the receipt and its charge.
        relay.send(
            format!(
                ": stratum-api receipt {} charge {charge} {}\n\n",
                admitted.id, admitted.currency
            )
            .as_bytes(),
        );
        return;
    }
    // Nothing streamed: the provider refused or failed before replying with events.
    let status = match ran.status {
        Some(st) if st != 200 => st,
        _ if ran.reason == "timeout" => 504,
        _ => 502,
    };
    let mut provider_error = keys::provider_error(&relay.held, &key);
    provider_error["error"]["type"] = json!(keys::http_failure_code(status));
    let (message, code) = provider_failure(&provider_error);
    respond(
        relay.s,
        status,
        &error(
            &message,
            &code,
            json!({"receipt":admitted.id,"charge":charge,"currency":admitted.currency,"basis":settled.basis}),
        ),
    );
}

fn provider_failure(clean: &Value) -> (String, String) {
    let message = clean["error"]["message"]
        .as_str()
        .unwrap_or("the provider call failed")
        .to_owned();
    let code = clean["error"]["type"]
        .as_str()
        .or(clean["error"]["code"].as_str())
        .unwrap_or("provider_error")
        .to_owned();
    (message, code)
}

fn safe_frame(event: &sse::Event) -> String {
    let value: Value = serde_json::from_str(&event.data).unwrap_or(Value::Null);
    let kind = value["type"].as_str().unwrap_or(&event.name);
    if matches!(event.name.as_str(), "error" | "response.failed")
        || matches!(kind, "error" | "response.failed")
        || value["error"].is_object()
        || value["response"]["status"] == "failed"
    {
        let code = keys::failure_code(
            value["response"]["error"]["code"]
                .as_str()
                .or(value["error"]["code"].as_str())
                .or(value["code"].as_str())
                .unwrap_or(""),
        );
        return sse::frame(
            "response.failed",
            &json!({"type":"response.failed","response":{"status":"failed",
            "error":{"code":code,"message":"the provider call failed"}}}),
        );
    }
    format!("event: {}\ndata: {}\n\n", event.name, event.data)
}

/// Bytes of the provider's stream: OpenAI's go to Zeno unchanged (and are watched for the usage);
/// Anthropic's are translated event by event.
fn pass(
    relay: &mut Relay,
    parser: &mut sse::Parser,
    watch: &mut openai::Watch,
    tr: Option<&mut anthropic::Translator>,
    b: &[u8],
) -> bool {
    let events = parser.feed(b);
    match tr {
        None => {
            let mut ok = true;
            for e in &events {
                watch.see(e);
                ok = relay.send(safe_frame(e).as_bytes()) && ok;
            }
            ok
        }
        Some(t) => {
            let mut ok = true;
            for e in &events {
                for (k, v) in t.feed(e) {
                    ok = relay.send(sse::frame(&k, &v).as_bytes());
                }
            }
            ok
        }
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    #[test]
    fn streamed_error_frames_cannot_echo_partial_or_preprocessed_provider_fields() {
        for text in [
            "sk-fixture-PREFIX!TAIL-SENTINEL",
            "***!TAIL-SENTINEL",
            "TAIL-SENTINEL",
        ] {
            for kind in ["error", "response.failed"] {
                let event=sse::Event {name:kind.into(),data:json!({"type":kind,"code":text,
                    "error":{"code":text,"message":text},"response":{"error":{"code":text,"message":text}}}).to_string()};
                let frame = safe_frame(&event);
                assert!(!frame.contains("SENTINEL"));
                assert!(frame.contains("the provider call failed"));
                let forged = sse::Event {
                    name: kind.into(),
                    data: json!({"type":text,"error":{"code":text,"message":text}}).to_string(),
                };
                assert!(!safe_frame(&forged).contains("SENTINEL"));
            }
        }
    }
    #[test]
    fn responses_failure_fields_are_extracted_only_after_credential_suppression() {
        let key = keys::Secret::local("sk-fixture-PREFIX!TAIL-SENTINEL".into());
        let raw = json!({"error":{"message":key.expose(),"type":key.expose(),"code":key.expose()}})
            .to_string();
        let (message, code) = provider_failure(&keys::provider_error(raw.as_bytes(), &key));
        assert_eq!(message, "the provider call failed");
        assert_eq!(code, "provider_error");
        let serialized = error(&message, &code, Value::Null).to_string();
        assert!(!serialized.contains("TAIL-SENTINEL"));
        assert_eq!(
            provider_failure(&keys::provider_error(
                b"unreadable sk-fixture-PREFIX!TAIL-SENTINEL",
                &key
            )),
            ("the provider call failed".into(), "provider_error".into())
        );
    }
}
