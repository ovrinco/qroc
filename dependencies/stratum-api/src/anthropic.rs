//! Anthropic's own Messages API (`POST {base}/v1/messages`), spoken directly: system text at the top
//! level, `max_tokens` as the output cap (thinking included), effort in `output_config`, the reply
//! streamed so a long one keeps its connection alive (assembled in `stream.rs`).
use serde_json::{Value, json};

pub const VERSION_HEADER: &str = "anthropic-version: 2026-10-21";

/// The request body from the shim's OpenAI-form inputs. Leading system messages become the top-level
/// `system` text; a later system message stays in place (the API accepts one after a user turn).
pub fn body(r: &Value, messages: &Value, max_output: u64) -> Value {
    let mut system = Vec::new();
    let mut rest = Vec::new();
    for m in messages.as_array().into_iter().flatten() {
        if m["role"] == "system" && rest.is_empty() {
            system.push(
                m["content"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| m["content"].to_string()),
            );
        } else {
            rest.push(json!({"role":m["role"],"content":content(&m["content"])}));
        }
    }
    let mut b = json!({"model":r["model"],"max_tokens":max_output,"messages":rest,"stream":true});
    if !system.is_empty() {
        b["system"] = json!(system.join("\n\n"));
    }
    if let Some(e) = r["effort"].as_str() {
        b["output_config"] = json!({"effort":e});
    }
    b
}

/// OpenAI-form content: a string stays a string; text parts become text blocks and `image_url` data
/// URIs (the intake's PNG or JPEG frames) become base64 image blocks. Anything else is sent as text.
fn content(c: &Value) -> Value {
    let Some(parts) = c.as_array() else {
        return c
            .as_str()
            .map_or_else(|| json!(c.to_string()), |s| json!(s));
    };
    let blocks: Vec<Value> = parts
        .iter()
        .map(|p| match p["type"].as_str() {
            Some("text") => json!({"type":"text","text":p["text"]}),
            Some("image_url") => {
                let uri = p["image_url"]["url"].as_str().unwrap_or("");
                match uri.strip_prefix("data:").and_then(|u| u.split_once(";base64,")) {
                    Some((media, data)) => {
                        json!({"type":"image","source":{"type":"base64","media_type":media,"data":data}})
                    }
                    None => json!({"type":"text","text":p.to_string()}),
                }
            }
            _ => json!({"type":"text","text":p.to_string()}),
        })
        .collect();
    json!(blocks)
}

/// The reply in the shim's OpenAI form, keeping the provider's id, model, stop reason and usage.
/// None when it carries no text content at all.
pub fn normalise(v: &Value) -> Option<Value> {
    let blocks = v["content"].as_array()?;
    let text: String = blocks
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
    let stop = v["stop_reason"].as_str().unwrap_or("unknown");
    Some(json!({
        "id":v["id"],
        "model":v["model"],
        "choices":[{"message":{"content":text},"finish_reason":if stop=="max_tokens"{"length"}else{"stop"}}],
        "usage":v["usage"],
        "completion_reason":stop,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_lifted_and_reply_normalised() {
        let r = json!({"model":"claude-opus-5-5","effort":"medium"});
        let m = json!([{"role":"system","content":"persona"},{"role":"user","content":"idea"}]);
        let b = body(&r, &m, 900);
        assert_eq!(b["system"], "persona");
        assert_eq!(b["messages"], json!([{"role":"user","content":"idea"}]));
        assert_eq!(b["max_tokens"], 900);
        assert_eq!(b["stream"], true);
        assert_eq!(b["output_config"]["effort"], "medium");
        assert!(b.get("temperature").is_none());
        let m = json!([{"role":"user","content":[{"type":"text","text":"look"},{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBO"}}]}]);
        let b = body(&r, &m, 10);
        assert_eq!(
            b["messages"][0]["content"][1],
            json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBO"}})
        );
        assert_eq!(b["messages"][0]["content"][0]["text"], "look");
        let v = json!({"id":"msg_1","model":"claude-opus-5-5","content":[{"type":"thinking","thinking":""},{"type":"text","text":"a"},{"type":"text","text":"b"}],"stop_reason":"max_tokens","usage":{"input_tokens":3,"output_tokens":4}});
        let n = normalise(&v).unwrap();
        assert_eq!(n["choices"][0]["message"]["content"], "ab");
        assert_eq!(n["choices"][0]["finish_reason"], "length");
        assert_eq!(n["completion_reason"], "max_tokens");
        assert_eq!(n["id"], "msg_1");
    }
}
