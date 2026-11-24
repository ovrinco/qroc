//! Anthropic's Messages API behind the Responses protocol Zeno speaks. A request's instructions, input
//! items and tools become a Messages request; the Messages stream becomes the Responses events Zeno reads
//! (`response.created`, output items added and done, text and argument deltas, `response.completed` with
//! usage). Thinking blocks travel back to Zeno inside reasoning items (their `encrypted_content`) and are
//! returned to Anthropic unchanged on the next turn, so a tool loop keeps its thinking.
use super::sse::Event;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const VERSION_HEADER: &str = "anthropic-version: 2026-10-21";
/// Marks a reasoning item's `encrypted_content` as an Anthropic thinking block this layer wrote.
const THINKING_MARK: &str = "stratum-anthropic-thinking/1:";

/// A tool Zeno offered, as Anthropic sees it under a flat name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tool {
    Function {
        namespace: Option<String>,
        name: String,
    },
    /// A freeform tool (`custom`): its one string input travels as `{"input": "..."}`.
    Custom { name: String },
}

/// Flat name → the tool Zeno knows, built from the request's tool list.
pub type Tools = BTreeMap<String, Tool>;

/// Anthropic tool names allow letters, digits, `_` and `-`, up to 128 characters.
pub fn flat_name(namespace: Option<&str>, name: &str) -> String {
    let joined = match namespace {
        Some(ns) if !ns.is_empty() => format!("{ns}__{name}"),
        _ => name.to_owned(),
    };
    let mut s: String = joined
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s.truncate(128);
    if s.is_empty() { "tool".into() } else { s }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Responses content parts → Anthropic content blocks (text and images; anything else as its text).
fn blocks(content: &Value) -> Vec<Value> {
    let Some(parts) = content.as_array() else {
        let t = text_of(content);
        return if t.is_empty() {
            vec![]
        } else {
            vec![json!({"type":"text","text":t})]
        };
    };
    parts
        .iter()
        .filter_map(|p| match p["type"].as_str() {
            Some("input_text" | "output_text" | "text") => p["text"].as_str().filter(|t| !t.is_empty()).map(|t| json!({"type":"text","text":t})),
            Some("input_image") => {
                let uri = p["image_url"].as_str().unwrap_or("");
                Some(match uri.strip_prefix("data:").and_then(|u| u.split_once(";base64,")) {
                    Some((media, data)) => json!({"type":"image","source":{"type":"base64","media_type":media,"data":data}}),
                    None if !uri.is_empty() => json!({"type":"image","source":{"type":"url","url":uri}}),
                    None => return None,
                })
            }
            Some("refusal") => p["refusal"].as_str().map(|t| json!({"type":"text","text":t})),
            _ => None,
        })
        .collect()
}

/// A tool's output (string or content parts) as tool_result content.
fn tool_output(output: &Value) -> Value {
    match output {
        Value::String(s) => json!(s),
        Value::Array(_) => {
            let b = blocks(output);
            if b.is_empty() { json!("") } else { json!(b) }
        }
        Value::Object(o) if o.contains_key("content") => tool_output(&output["content"]),
        other => json!(other.to_string()),
    }
}

/// The messages being built: consecutive blocks of one role are merged into one message.
#[derive(Default)]
struct Messages(Vec<Value>);
impl Messages {
    fn push(&mut self, role: &str, mut new: Vec<Value>) {
        if new.is_empty() {
            return;
        }
        if let Some(last) = self.0.last_mut()
            && last["role"] == role
            && let Some(c) = last["content"].as_array_mut()
        {
            c.append(&mut new);
            return;
        }
        self.0.push(json!({"role":role,"content":new}));
    }
}

/// The thinking block a reasoning item carries, when this layer wrote it.
fn thinking_block(item: &Value) -> Option<Value> {
    let enc = item["encrypted_content"]
        .as_str()?
        .strip_prefix(THINKING_MARK)?;
    serde_json::from_str::<Value>(enc)
        .ok()
        .filter(|b| b.is_object())
}

/// The cache mark: ephemeral, Anthropic's 5-minute default.
fn cache() -> Value {
    json!({"type":"ephemeral"})
}

/// The mark on the conversation's growing end: the last block of the last message that may carry one.
fn mark_last_cacheable(messages: &mut [Value]) {
    let Some(last) = messages.last_mut() else {
        return;
    };
    if let Some(block) = last["content"].as_array_mut().and_then(|c| {
        c.iter_mut()
            .rev()
            .find(|b| !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")))
    }) {
        block["cache_control"] = cache();
    }
}

/// The Messages request for Zeno's Responses request, and the tools it offers by flat name.
pub fn request(body: &Value, model: &str, max_output: u64, effort: Option<&str>) -> (Value, Tools) {
    let mut system: Vec<String> = Vec::new();
    if let Some(s) = body["instructions"].as_str().filter(|s| !s.is_empty()) {
        system.push(s.to_owned());
    }
    let mut tools = Tools::new();
    let mut specs = Vec::new();
    let mut add = |flat: String,
                   tool: Tool,
                   description: &Value,
                   schema: Value,
                   tools: &mut Tools| {
        if tools.contains_key(&flat) {
            return;
        }
        specs.push(json!({"name":flat,"description":description.as_str().unwrap_or(""),"input_schema":schema}));
        tools.insert(flat, tool);
    };
    let schema = |p: &Value| {
        if p.is_object() {
            p.clone()
        } else {
            json!({"type":"object","properties":{}})
        }
    };
    for t in body["tools"].as_array().into_iter().flatten() {
        match t["type"].as_str() {
            Some("function") => {
                let name = t["name"].as_str().unwrap_or_default();
                add(
                    flat_name(None, name),
                    Tool::Function {
                        namespace: None,
                        name: name.into(),
                    },
                    &t["description"],
                    schema(&t["parameters"]),
                    &mut tools,
                );
            }
            Some("namespace") => {
                let ns = t["name"].as_str().unwrap_or_default();
                for f in t["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| f["type"] == "function")
                {
                    let name = f["name"].as_str().unwrap_or_default();
                    add(
                        flat_name(Some(ns), name),
                        Tool::Function {
                            namespace: Some(ns.into()),
                            name: name.into(),
                        },
                        &f["description"],
                        schema(&f["parameters"]),
                        &mut tools,
                    );
                }
            }
            Some("custom") => {
                let name = t["name"].as_str().unwrap_or_default();
                let mut d = t["description"].as_str().unwrap_or("").to_owned();
                if let Some(def) = t["format"]["definition"].as_str() {
                    d.push_str(&format!("\n\nPut the whole input in the `input` string. It must follow this {} grammar:\n{def}", t["format"]["syntax"].as_str().unwrap_or("")));
                }
                add(
                    flat_name(None, name),
                    Tool::Custom { name: name.into() },
                    &json!(d),
                    json!({"type":"object","properties":{"input":{"type":"string"}},"required":["input"]}),
                    &mut tools,
                );
            }
            // Server-side tools of OpenAI (web search, tool search) have no Anthropic counterpart here.
            _ => {}
        }
    }

    let mut msgs = Messages::default();
    let mut seen_turn = false;
    for item in body["input"].as_array().into_iter().flatten() {
        let kind = item["type"]
            .as_str()
            .unwrap_or(if item["role"].is_string() {
                "message"
            } else {
                ""
            });
        match kind {
            "message" => {
                let role = item["role"].as_str().unwrap_or("user");
                match role {
                    "system" | "developer" if !seen_turn => {
                        let t = text_of(&item["content"]);
                        if !t.is_empty() {
                            system.push(t);
                        }
                    }
                    // A later operator note joins the user's side of the conversation as text.
                    "system" | "developer" => msgs.push("user", blocks(&item["content"])),
                    "assistant" => {
                        seen_turn = true;
                        msgs.push("assistant", blocks(&item["content"]));
                    }
                    _ => {
                        seen_turn = true;
                        msgs.push("user", blocks(&item["content"]));
                    }
                }
            }
            "reasoning" => {
                if let Some(b) = thinking_block(item) {
                    seen_turn = true;
                    msgs.push("assistant", vec![b]);
                }
            }
            "function_call" => {
                seen_turn = true;
                let flat = flat_name(
                    item["namespace"].as_str(),
                    item["name"].as_str().unwrap_or_default(),
                );
                let input = item["arguments"]
                    .as_str()
                    .and_then(|a| serde_json::from_str::<Value>(a).ok())
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                msgs.push(
                    "assistant",
                    vec![json!({"type":"tool_use","id":item["call_id"],"name":flat,"input":input})],
                );
            }
            "custom_tool_call" => {
                seen_turn = true;
                let flat = flat_name(None, item["name"].as_str().unwrap_or_default());
                msgs.push("assistant", vec![json!({"type":"tool_use","id":item["call_id"],"name":flat,"input":{"input":item["input"]}})]);
            }
            "function_call_output" | "custom_tool_call_output" => {
                seen_turn = true;
                msgs.push("user", vec![json!({"type":"tool_result","tool_use_id":item["call_id"],"content":tool_output(&item["output"])})]);
            }
            _ => {}
        }
    }

    // Prompt caching: three breakpoints of Anthropic's four, 5-minute cache. The prefix renders as tools, then
    // system, then messages; Zeno's history only grows, so each turn reads what the last one wrote and writes
    // its own end. Thinking blocks cannot carry a mark; the last block that can, does.
    let mut messages = msgs.0;
    mark_last_cacheable(&mut messages);
    if let Some(last) = specs.last_mut() {
        last["cache_control"] = cache();
    }
    let mut b = json!({"model":model,"max_tokens":max_output,"messages":messages,"stream":true,"thinking":{"type":"adaptive"}});
    if !system.is_empty() {
        b["system"] = json!([{"type":"text","text":system.join("\n\n"),"cache_control":cache()}]);
    }
    if let Some(e) = effort {
        b["output_config"] = json!({"effort":e});
    }
    if !specs.is_empty() {
        b["tools"] = json!(specs);
        if body["parallel_tool_calls"] == json!(false) {
            b["tool_choice"] = json!({"type":"auto","disable_parallel_tool_use":true});
        }
    }
    (b, tools)
}

/// One content block being streamed.
struct Block {
    kind: String,
    item_id: String,
    output_index: usize,
    raw: Value,
    text: String,
}

/// The Messages stream turned into Responses events, one Anthropic event at a time.
pub struct Translator {
    tools: Tools,
    seq: u64,
    id: String,
    model: Value,
    created: bool,
    blocks: BTreeMap<u64, Block>,
    output: Vec<Value>,
    /// The Anthropic message as it would have come whole (usage overlaid), for settlement.
    pub message: Value,
    pub usage_final: bool,
    pub stopped: bool,
    pub error: Option<String>,
    pub finished: bool,
}

impl Translator {
    pub fn new(tools: Tools) -> Self {
        Self {
            tools,
            seq: 0,
            id: String::new(),
            model: Value::Null,
            created: false,
            blocks: BTreeMap::new(),
            output: Vec::new(),
            message: json!({}),
            usage_final: false,
            stopped: false,
            error: None,
            finished: false,
        }
    }

    fn emit(&mut self, out: &mut Vec<(String, Value)>, kind: &str, mut v: Value) {
        v["type"] = json!(kind);
        v["sequence_number"] = json!(self.seq);
        self.seq += 1;
        out.push((kind.to_owned(), v));
    }

    fn response(&self, status: &str) -> Value {
        json!({"id":self.id,"object":"response","model":self.model,"status":status,"output":self.output})
    }

    /// Responses usage from the Anthropic usage: all input counted (cache reads named as cached), output
    /// with thinking included.
    fn usage(&self) -> Value {
        let u = &self.message["usage"];
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        let input =
            n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
        let output = n("output_tokens");
        json!({"input_tokens":input,"input_tokens_details":{"cached_tokens":n("cache_read_input_tokens")},
               "output_tokens":output,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":input + output})
    }

    /// Feed one Anthropic event; returns the Responses events (name, payload) it yields.
    pub fn feed(&mut self, e: &Event) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        let Ok(v) = serde_json::from_str::<Value>(&e.data) else {
            return out;
        };
        match v["type"].as_str().unwrap_or(e.name.as_str()) {
            "message_start" => {
                self.message = v["message"].clone();
                self.id = format!(
                    "resp_{}",
                    v["message"]["id"].as_str().unwrap_or("anthropic")
                );
                self.model = v["message"]["model"].clone();
                if !self.created {
                    self.created = true;
                    let r = self.response("in_progress");
                    self.emit(&mut out, "response.created", json!({"response":r}));
                }
            }
            "content_block_start" => {
                let index = v["index"].as_u64().unwrap_or(self.blocks.len() as u64);
                let cb = v["content_block"].clone();
                let kind = cb["type"].as_str().unwrap_or("").to_owned();
                let output_index = self.output.len() + self.blocks.len();
                let item_id = format!(
                    "{}_{}_{index}",
                    match kind.as_str() {
                        "text" => "msg",
                        "tool_use" => "fc",
                        _ => "rs",
                    },
                    self.id.trim_start_matches("resp_")
                );
                let added = match kind.as_str() {
                    "text" => Some(
                        json!({"type":"message","id":item_id,"role":"assistant","status":"in_progress","content":[]}),
                    ),
                    "tool_use" => Some(self.call_item(&cb, &item_id, "", "in_progress")),
                    "thinking" | "redacted_thinking" => {
                        Some(json!({"type":"reasoning","id":item_id,"summary":[]}))
                    }
                    _ => None,
                };
                if let Some(item) = added {
                    self.emit(
                        &mut out,
                        "response.output_item.added",
                        json!({"output_index":output_index,"item":item}),
                    );
                }
                let text = cb["text"].as_str().unwrap_or("").to_owned();
                self.blocks.insert(
                    index,
                    Block {
                        kind,
                        item_id,
                        output_index,
                        raw: cb,
                        text,
                    },
                );
            }
            "content_block_delta" => {
                let index = v["index"].as_u64().unwrap_or(0);
                let d = &v["delta"];
                let Some(b) = self.blocks.get_mut(&index) else {
                    return out;
                };
                let (field, part) = match d["type"].as_str() {
                    Some("text_delta") => ("text", d["text"].as_str().unwrap_or("")),
                    Some("thinking_delta") => ("thinking", d["thinking"].as_str().unwrap_or("")),
                    Some("signature_delta") => ("signature", d["signature"].as_str().unwrap_or("")),
                    Some("input_json_delta") => {
                        ("partial_json", d["partial_json"].as_str().unwrap_or(""))
                    }
                    _ => return out,
                };
                if field == "text" || field == "partial_json" {
                    b.text.push_str(part);
                } else {
                    let s = format!("{}{}", b.raw[field].as_str().unwrap_or(""), part);
                    b.raw[field] = json!(s);
                }
                let (item_id, output_index) = (b.item_id.clone(), b.output_index);
                if field == "text" {
                    self.emit(&mut out, "response.output_text.delta", json!({"item_id":item_id,"output_index":output_index,"content_index":0,"delta":part}));
                } else if field == "partial_json" {
                    self.emit(
                        &mut out,
                        "response.function_call_arguments.delta",
                        json!({"item_id":item_id,"output_index":output_index,"delta":part}),
                    );
                }
            }
            "content_block_stop" => {
                let index = v["index"].as_u64().unwrap_or(0);
                if let Some(b) = self.blocks.remove(&index) {
                    if let Some(item) = self.done_item(&b) {
                        self.output.push(item.clone());
                        self.emit(
                            &mut out,
                            "response.output_item.done",
                            json!({"output_index":b.output_index,"item":item}),
                        );
                    }
                    self.keep_block(&b);
                }
            }
            "message_delta" => {
                for k in ["stop_reason", "stop_sequence"] {
                    if !v["delta"][k].is_null() {
                        self.message[k] = v["delta"][k].clone();
                    }
                }
                if let Some(u) = v["usage"].as_object() {
                    if !self.message["usage"].is_object() {
                        self.message["usage"] = json!({});
                    }
                    for (k, x) in u {
                        if !x.is_null() {
                            self.message["usage"][k] = x.clone();
                        }
                    }
                    self.usage_final = self.message["usage"]["output_tokens"].is_u64();
                }
                self.stopped = self.message["stop_reason"].is_string();
            }
            "message_stop" => {
                self.finished = true;
                let mut r = self.response("completed");
                r["usage"] = self.usage();
                if self.message["stop_reason"] == "max_tokens" {
                    r["status"] = json!("incomplete");
                    r["incomplete_details"] = json!({"reason":"max_output_tokens"});
                    self.emit(&mut out, "response.incomplete", json!({"response":r}));
                } else {
                    self.emit(&mut out, "response.completed", json!({"response":r}));
                }
            }
            "error" => {
                self.error = Some("provider stream error".into());
                let code = crate::keys::failure_code(v["error"]["type"].as_str().unwrap_or(""));
                out.extend(self.failed(code, "the provider call failed"));
            }
            _ => {}
        }
        out
    }

    /// A `response.failed` event (for a provider error, or a stream cut short).
    pub fn failed(&mut self, code: &str, message: &str) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        let mut r = self.response("failed");
        let _ = message;
        let code = crate::keys::failure_code(code);
        r["error"] = json!({"code":code,"message":"the provider call failed"});
        self.emit(&mut out, "response.failed", json!({"response":r}));
        out
    }

    fn call_item(&self, cb: &Value, item_id: &str, arguments: &str, status: &str) -> Value {
        let flat = cb["name"].as_str().unwrap_or_default();
        match self.tools.get(flat) {
            Some(Tool::Custom { name }) => {
                let input = serde_json::from_str::<Value>(arguments)
                    .ok()
                    .and_then(|v| v["input"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                json!({"type":"custom_tool_call","id":item_id,"call_id":cb["id"],"name":name,"input":input,"status":status})
            }
            Some(Tool::Function { namespace, name }) => {
                let mut i = json!({"type":"function_call","id":item_id,"call_id":cb["id"],"name":name,"arguments":arguments,"status":status});
                if let Some(ns) = namespace {
                    i["namespace"] = json!(ns);
                }
                i
            }
            None => {
                json!({"type":"function_call","id":item_id,"call_id":cb["id"],"name":flat,"arguments":arguments,"status":status})
            }
        }
    }

    fn done_item(&self, b: &Block) -> Option<Value> {
        match b.kind.as_str() {
            "text" => Some(
                json!({"type":"message","id":b.item_id,"role":"assistant","status":"completed",
                                   "content":[{"type":"output_text","text":b.text,"annotations":[]}]}),
            ),
            "tool_use" => {
                let args = if b.text.trim().is_empty() {
                    "{}".to_owned()
                } else {
                    b.text.clone()
                };
                Some(self.call_item(&b.raw, &b.item_id, &args, "completed"))
            }
            "thinking" | "redacted_thinking" => {
                let summary: Vec<Value> = b.raw["thinking"]
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .map(|t| vec![json!({"type":"summary_text","text":t})])
                    .unwrap_or_default();
                Some(json!({"type":"reasoning","id":b.item_id,"summary":summary,
                            "encrypted_content":format!("{THINKING_MARK}{}", b.raw)}))
            }
            _ => None,
        }
    }

    /// The block as the whole message would carry it, for settlement and the kept reply.
    fn keep_block(&mut self, b: &Block) {
        let mut raw = b.raw.clone();
        match b.kind.as_str() {
            "text" => raw["text"] = json!(b.text),
            "tool_use" => {
                raw["input"] =
                    serde_json::from_str::<Value>(&b.text).unwrap_or_else(|_| json!(b.text))
            }
            _ => {}
        }
        if !self.message["content"].is_array() {
            self.message["content"] = json!([]);
        }
        if let Some(c) = self.message["content"].as_array_mut() {
            c.push(raw);
        }
    }
}

/// The settle-ready reply (an Anthropic message with its final usage), or None when the usage never came.
pub fn settlement(t: &Translator) -> Option<Value> {
    t.usage_final.then(|| t.message.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_flattened_within_rules() {
        assert_eq!(
            flat_name(Some("mcp__ovrin-recall"), "recall"),
            "mcp__ovrin-recall__recall"
        );
        assert_eq!(flat_name(None, "a.b c"), "a_b_c");
        assert_eq!(flat_name(None, &"x".repeat(200)).len(), 128);
    }
}
