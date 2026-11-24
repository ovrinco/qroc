//! Streamed replies (server-sent events), assembled back into the provider's non-streamed reply so
//! settlement reads the same fields (lifted from route-shim `stream.rs`). A stream counts as finished
//! only when its final usage and stop reason arrived; a stream cut before that leaves the charge unknown.
use serde_json::{Map, Value, json};

/// One assembled stream: the reply as the provider would have sent it whole, whether its final usage
/// arrived (the charge is then exact), whether it ended with a stop reason, and any error event.
pub struct Assembled {
    pub reply: Value,
    pub usage_final: bool,
    pub stopped: bool,
    pub error: Option<String>,
}

impl Assembled {
    /// The reply is whole: usage and stop reason both arrived and no error event came.
    pub fn finished(&self) -> bool {
        self.usage_final && self.whole()
    }
    /// The content is whole: a stop reason arrived and no error event came (the usage may not have).
    pub fn whole(&self) -> bool {
        self.stopped && self.error.is_none()
    }
}

/// The `data:` payloads of a server-sent event stream, with their event names (multi-line data joined
/// by newlines; comments and keep-alive lines skipped).
pub fn events(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut name, mut data) = (String::new(), Vec::<&str>::new());
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        if line.is_empty() {
            if !data.is_empty() {
                out.push((std::mem::take(&mut name), data.join("\n")));
            }
            name.clear();
            data.clear();
        } else if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v));
        } else if let Some(v) = line.strip_prefix("event:") {
            name = v.trim().to_owned();
        }
    }
    // A cut stream may end without its blank line; its last complete-looking event still counts only
    // if its data parses, which the callers check.
    if !data.is_empty() {
        out.push((name, data.join("\n")));
    }
    out
}

/// True when the body is a stream rather than one JSON reply (a provider may ignore `stream`).
pub fn is_stream(text: &str) -> bool {
    !text.trim_start().starts_with('{')
}

/// OpenAI chat completions with `stream_options.include_usage`: content deltas, a finish reason, then
/// one chunk with empty choices carrying the usage, then `[DONE]`.
pub fn openai(text: &str) -> Assembled {
    let (mut content, mut finish, mut usage, mut error) =
        (String::new(), Value::Null, Value::Null, None);
    let mut head = Map::new();
    for (_, data) in events(text) {
        if data.trim() == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if v.get("error").is_some() {
            error = Some("provider stream error".into());
            continue;
        }
        for k in [
            "id",
            "model",
            "service_tier",
            "created",
            "system_fingerprint",
        ] {
            if !v[k].is_null() {
                head.insert(k.into(), v[k].clone());
            }
        }
        if let Some(c) = v["choices"].get(0) {
            if let Some(t) = c["delta"]["content"].as_str() {
                content.push_str(t);
            }
            if !c["finish_reason"].is_null() {
                finish = c["finish_reason"].clone();
            }
        }
        if v["usage"].is_object() {
            usage = v["usage"].clone();
        }
    }
    let mut reply = Value::Object(head);
    reply["object"] = json!("chat.completion");
    reply["choices"] = json!([{"index":0,"message":{"role":"assistant","content":content},"finish_reason":finish}]);
    let usage_final = usage.is_object();
    if usage_final {
        reply["usage"] = usage;
    }
    Assembled {
        reply,
        usage_final,
        stopped: !finish.is_null(),
        error,
    }
}

/// Anthropic Messages: `message_start` (id, model, input usage), content blocks built from deltas,
/// `message_delta` (stop reason and the final usage, which overlays the start's), `message_stop`.
pub fn anthropic(text: &str) -> Assembled {
    let mut msg = json!({});
    let mut blocks: Vec<Value> = Vec::new();
    let (mut usage_final, mut error) = (false, None);
    for (name, data) in events(text) {
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        match v["type"].as_str().unwrap_or(name.as_str()) {
            "message_start" => msg = v["message"].clone(),
            "content_block_start" => {
                let i = v["index"].as_u64().unwrap_or(blocks.len() as u64) as usize;
                if blocks.len() <= i {
                    blocks.resize(i + 1, Value::Null);
                }
                blocks[i] = v["content_block"].clone();
            }
            "content_block_delta" => {
                let i = v["index"].as_u64().unwrap_or(0) as usize;
                let Some(b) = blocks.get_mut(i) else { continue };
                let d = &v["delta"];
                let (field, part) = match d["type"].as_str() {
                    Some("text_delta") => ("text", &d["text"]),
                    Some("thinking_delta") => ("thinking", &d["thinking"]),
                    Some("signature_delta") => ("signature", &d["signature"]),
                    Some("input_json_delta") => ("partial_json", &d["partial_json"]),
                    _ => continue,
                };
                let s = format!(
                    "{}{}",
                    b[field].as_str().unwrap_or(""),
                    part.as_str().unwrap_or("")
                );
                b[field] = json!(s);
            }
            "message_delta" => {
                for k in ["stop_reason", "stop_sequence"] {
                    if !v["delta"][k].is_null() {
                        msg[k] = v["delta"][k].clone();
                    }
                }
                if let Some(u) = v["usage"].as_object() {
                    if !msg["usage"].is_object() {
                        msg["usage"] = json!({});
                    }
                    for (k, x) in u {
                        if !x.is_null() {
                            msg["usage"][k] = x.clone();
                        }
                    }
                    usage_final = msg["usage"]["output_tokens"].is_u64();
                }
            }
            "error" => {
                error = Some("provider stream error".into());
            }
            _ => {}
        }
    }
    let stopped = msg["stop_reason"].is_string();
    if msg.is_object() {
        msg["content"] = json!(
            blocks
                .into_iter()
                .filter(|b| !b.is_null())
                .collect::<Vec<_>>()
        );
    }
    Assembled {
        reply: msg,
        usage_final,
        stopped,
        error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assembled_stream_errors_never_retain_provider_controlled_fields() {
        for text in [
            "sk-fixture-PREFIX!TAIL-SENTINEL",
            "***!TAIL-SENTINEL",
            "TAIL-SENTINEL",
        ] {
            let open = format!("data: {}\n\n", json!({"error":{"message":text}}));
            assert_eq!(
                openai(&open).error.as_deref(),
                Some("provider stream error")
            );
            let anthropic = format!(
                "event: error\ndata: {}\n\n",
                json!({"type":"error","error":{"type":text,"message":text}})
            );
            assert_eq!(
                super::anthropic(&anthropic).error.as_deref(),
                Some("provider stream error")
            );
        }
    }
    #[test]
    fn openai_stream_assembled_and_cut_detected() {
        let s = "data: {\"id\":\"c1\",\"model\":\"m-1\",\"service_tier\":\"default\",\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\n\
                 : keep-alive\n\n\
                 data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"content\":\"llo\"},\"finish_reason\":\"stop\"}]}\n\n\
                 data: {\"id\":\"c1\",\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
        let a = openai(s);
        assert!(a.finished());
        assert_eq!(a.reply["choices"][0]["message"]["content"], "hello");
        assert_eq!(a.reply["usage"]["completion_tokens"], 2);
        assert_eq!(a.reply["service_tier"], "default");
        let cut = openai(&s[..s.find("data: {\"id\":\"c1\",\"choices\":[]").unwrap()]);
        assert!(!cut.usage_final && cut.stopped && !cut.finished());
    }
    #[test]
    fn anthropic_stream_assembled_and_cut_detected() {
        let s = "event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"c\",\"content\":[],\"usage\":{\"input_tokens\":7,\"output_tokens\":1,\"service_tier\":\"standard\"}}}\r\n\r\n\
                 event: ping\r\ndata: {\"type\":\"ping\"}\r\n\r\n\
                 event: content_block_start\r\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\r\n\r\n\
                 event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hm\"}}\r\n\r\n\
                 event: content_block_start\r\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\r\n\r\n\
                 event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"a\"}}\r\n\r\n\
                 event: content_block_delta\r\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"b\"}}\r\n\r\n\
                 event: message_delta\r\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":9}}\r\n\r\n\
                 event: message_stop\r\ndata: {\"type\":\"message_stop\"}\r\n\r\n";
        let a = anthropic(s);
        assert!(a.finished());
        assert_eq!(a.reply["usage"]["input_tokens"], 7);
        assert_eq!(
            a.reply["usage"]["output_tokens"], 9,
            "the final count overlays the start's"
        );
        assert_eq!(a.reply["usage"]["service_tier"], "standard");
        assert_eq!(a.reply["content"][1]["text"], "ab");
        assert_eq!(a.reply["content"][0]["thinking"], "hm");
        let cut = anthropic(&s[..s.find("event: message_delta").unwrap()]);
        assert!(
            !cut.usage_final && !cut.finished(),
            "start's output_tokens 1 is not the charge"
        );
        let err = anthropic(&format!(
            "{}event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\"}}}}\n\n",
            &s[..s.find("event: message_delta").unwrap()]
        ));
        assert!(err.error.is_some() && !err.finished());
    }
}
