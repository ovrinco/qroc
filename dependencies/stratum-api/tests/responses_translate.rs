//! The Responses ↔ Messages translation on its own: a text reply, and a two-turn tool loop whose second
//! request carries the first turn's thinking, tool call and tool result back as Anthropic expects them.
use serde_json::{Value, json};
use stratum_api::responses::{anthropic, sse};

fn stream(events: &[Value]) -> String {
    events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
        .collect()
}

fn run(t: &mut anthropic::Translator, text: &str) -> Vec<(String, Value)> {
    let mut p = sse::Parser::default();
    p.feed(text.as_bytes())
        .iter()
        .flat_map(|e| t.feed(e))
        .collect()
}

fn zeno_request(input: Value) -> Value {
    json!({
        "model": "claude-opus-5-5", "instructions": "You are Zeno.", "input": input,
        "tools": [
            {"type":"function","name":"shell_command","description":"Run a command","parameters":{"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}},
            {"type":"namespace","name":"mcp__ovrin-recall","description":"recall","tools":[{"type":"function","name":"recall","description":"Recall","parameters":{"type":"object","properties":{}}}]},
            {"type":"custom","name":"apply_patch","description":"Patch files","format":{"type":"grammar","syntax":"lark","definition":"start: x"}},
            {"type":"web_search"}
        ],
        "tool_choice": "auto", "parallel_tool_calls": false, "reasoning": {"effort":"medium"},
        "store": false, "stream": true, "include": ["reasoning.encrypted_content"]
    })
}

pub fn tool_turn() -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"id":"msg_1","model":"claude-opus-5-5","content":[],"usage":{"input_tokens":900,"cache_read_input_tokens":100,"output_tokens":1,"service_tier":"standard"}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Need the file."}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"SIG1"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"shell_command","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"cat a.txt\"}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":40}}),
        json!({"type":"message_stop"}),
    ]
}

pub fn text_turn(id: &str, text: &str) -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"id":id,"model":"claude-opus-5-5","content":[],"usage":{"input_tokens":1200,"output_tokens":1,"service_tier":"standard"}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":&text[..2]}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":&text[2..]}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}),
        json!({"type":"message_stop"}),
    ]
}

#[test]
fn text_reply_becomes_responses_events() {
    let (body, tools) = anthropic::request(
        &zeno_request(
            json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}]),
        ),
        "claude-opus-5-5",
        8000,
        Some("medium"),
    );
    assert_eq!(body["system"][0]["text"], "You are Zeno.");
    assert_eq!(body["max_tokens"], 8000);
    assert_eq!(body["output_config"]["effort"], "medium");
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(
        body["messages"],
        json!([{"role":"user","content":[{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]}])
    );
    assert_eq!(
        body["tool_choice"],
        json!({"type":"auto","disable_parallel_tool_use":true})
    );
    let names: Vec<&str> = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["shell_command", "mcp__ovrin-recall__recall", "apply_patch"],
        "web search has no counterpart and is left out"
    );
    let mut t = anthropic::Translator::new(tools);
    let out = run(&mut t, &stream(&text_turn("msg_2", "Hello there")));
    let kinds: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "response.created",
            "response.output_item.added",
            "response.output_text.delta",
            "response.output_text.delta",
            "response.output_item.done",
            "response.completed"
        ]
    );
    let (_, done) = &out[4];
    assert_eq!(
        done["item"]["content"][0],
        json!({"type":"output_text","text":"Hello there","annotations":[]})
    );
    let (_, completed) = &out[5];
    assert_eq!(completed["response"]["usage"]["input_tokens"], 1200);
    assert_eq!(completed["response"]["usage"]["output_tokens"], 7);
    assert_eq!(completed["response"]["output"].as_array().unwrap().len(), 1);
    let seqs: Vec<u64> = out
        .iter()
        .map(|(_, v)| v["sequence_number"].as_u64().unwrap())
        .collect();
    assert_eq!(seqs, (0..6).collect::<Vec<_>>());
    let s = anthropic::settlement(&t).unwrap();
    assert_eq!(s["usage"]["output_tokens"], 7);
    assert_eq!(s["content"][0]["text"], "Hello there");
}

#[test]
fn two_turn_tool_loop_round_trips() {
    let first = json!([{"type":"message","role":"developer","content":[{"type":"input_text","text":"Sandbox: full."}]},
                       {"type":"message","role":"user","content":[{"type":"input_text","text":"What is in a.txt?"}]}]);
    let (body, tools) = anthropic::request(
        &zeno_request(first.clone()),
        "claude-opus-5-5",
        8000,
        Some("medium"),
    );
    assert_eq!(
        body["system"][0]["text"], "You are Zeno.\n\nSandbox: full.",
        "a leading developer note joins the system text"
    );
    let mut t = anthropic::Translator::new(tools);
    let out = run(&mut t, &stream(&tool_turn()));
    let done: Vec<&Value> = out
        .iter()
        .filter(|(k, _)| k == "response.output_item.done")
        .map(|(_, v)| &v["item"])
        .collect();
    assert_eq!(done.len(), 2);
    assert_eq!(done[0]["type"], "reasoning");
    assert_eq!(done[0]["summary"][0]["text"], "Need the file.");
    assert_eq!(done[1]["type"], "function_call");
    assert_eq!(done[1]["name"], "shell_command");
    assert_eq!(done[1]["call_id"], "toolu_1");
    assert_eq!(
        serde_json::from_str::<Value>(done[1]["arguments"].as_str().unwrap()).unwrap(),
        json!({"command":"cat a.txt"})
    );
    assert!(out.iter().any(
        |(k, v)| k == "response.function_call_arguments.delta" && v["delta"] == "{\"command\":"
    ));
    let (_, completed) = out.last().unwrap();
    assert_eq!(completed["type"], "response.completed");
    assert_eq!(
        completed["response"]["usage"],
        json!({"input_tokens":1000,"input_tokens_details":{"cached_tokens":100},"output_tokens":40,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":1040})
    );

    // Turn two: Zeno sends the history back with the tool's output, as it records items.
    let mut second = first.as_array().unwrap().clone();
    second.push(done[0].clone());
    second.push(done[1].clone());
    second.push(json!({"type":"function_call_output","call_id":"toolu_1","output":"apples"}));
    let (body2, tools2) = anthropic::request(
        &zeno_request(json!(second)),
        "claude-opus-5-5",
        8000,
        Some("medium"),
    );
    assert_eq!(
        body2["messages"],
        json!([
            {"role":"user","content":[{"type":"text","text":"What is in a.txt?"}]},
            {"role":"assistant","content":[
                {"type":"thinking","thinking":"Need the file.","signature":"SIG1"},
                {"type":"tool_use","id":"toolu_1","name":"shell_command","input":{"command":"cat a.txt"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"apples","cache_control":{"type":"ephemeral"}}]}
        ])
    );
    let mut t2 = anthropic::Translator::new(tools2);
    let out2 = run(&mut t2, &stream(&text_turn("msg_3", "It holds apples.")));
    assert_eq!(out2.last().unwrap().0, "response.completed");
}

#[test]
fn namespaced_and_custom_calls_come_back_in_zeno_form() {
    let (_, tools) = anthropic::request(&zeno_request(json!([])), "claude-opus-5-5", 100, None);
    let mut t = anthropic::Translator::new(tools);
    let ev = vec![
        json!({"type":"message_start","message":{"id":"m","model":"claude-opus-5-5","usage":{"input_tokens":1,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_a","name":"mcp__ovrin-recall__recall","input":{}}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_b","name":"apply_patch","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"input\":\"*** Begin Patch\"}"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ];
    let out = run(&mut t, &stream(&ev));
    let done: Vec<&Value> = out
        .iter()
        .filter(|(k, _)| k == "response.output_item.done")
        .map(|(_, v)| &v["item"])
        .collect();
    assert_eq!(done[0]["name"], "recall");
    assert_eq!(done[0]["namespace"], "mcp__ovrin-recall");
    assert_eq!(done[0]["arguments"], "{}");
    assert_eq!(
        done[1],
        &json!({"type":"custom_tool_call","id":"fc_m_1","call_id":"toolu_b","name":"apply_patch","input":"*** Begin Patch","status":"completed"})
    );
    // And back: the custom call and its output return as tool_use / tool_result.
    let hist = json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"patch it"}]}, done[0], done[1],
        {"type":"function_call_output","call_id":"toolu_a","output":[{"type":"input_text","text":"remembered"}]},
        {"type":"custom_tool_call_output","call_id":"toolu_b","output":"Done"}]);
    let (b, _) = anthropic::request(&zeno_request(hist), "claude-opus-5-5", 100, None);
    assert_eq!(
        b["messages"][1]["content"][0]["name"],
        "mcp__ovrin-recall__recall"
    );
    assert_eq!(
        b["messages"][1]["content"][1]["input"],
        json!({"input":"*** Begin Patch"})
    );
    assert_eq!(
        b["messages"][2]["content"][0]["content"],
        json!([{"type":"text","text":"remembered"}])
    );
    assert_eq!(b["messages"][2]["content"][1]["content"], "Done");
}

#[test]
fn cut_stream_and_error_are_not_settled() {
    let (_, tools) = anthropic::request(&zeno_request(json!([])), "claude-opus-5-5", 100, None);
    let full = stream(&text_turn("m", "Hello"));
    let cut = &full[..full.find("event: message_delta").unwrap()];
    let mut t = anthropic::Translator::new(tools.clone());
    let out = run(&mut t, cut);
    assert!(!out.iter().any(|(k, _)| k == "response.completed"));
    assert!(
        anthropic::settlement(&t).is_none(),
        "the start's output count is not the charge"
    );
    let mut t = anthropic::Translator::new(tools);
    let err = format!(
        "{cut}event: error\ndata: {}\n\n",
        json!({"type":"error","error":{"type":"overloaded_error","message":"busy"}})
    );
    let out = run(&mut t, &err);
    assert_eq!(out.last().unwrap().0, "response.failed");
    assert_eq!(
        out.last().unwrap().1["response"]["error"]["code"],
        "overloaded_error"
    );
    assert!(t.error.is_some() && anthropic::settlement(&t).is_none());
}

#[test]
fn max_tokens_is_incomplete() {
    let mut ev = text_turn("m", "Hello");
    ev[5] = json!({"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":100}});
    let mut t = anthropic::Translator::new(Default::default());
    let out = run(&mut t, &stream(&ev));
    let (k, v) = out.last().unwrap();
    assert_eq!(k, "response.incomplete");
    assert_eq!(
        v["response"]["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert!(
        anthropic::settlement(&t).is_some(),
        "billed on its usage all the same"
    );
}

#[test]
fn cache_marks_on_tools_system_and_the_growing_end() {
    let hist = json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"first"}]},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"ok"}]},
        {"type":"message","role":"user","content":[{"type":"input_text","text":"second"}]}]);
    let (b, _) = anthropic::request(&zeno_request(hist.clone()), "claude-opus-5-5", 100, None);
    assert_eq!(
        b.to_string().matches("\"cache_control\"").count(),
        3,
        "tools, system and the conversation's end; within Anthropic's four"
    );
    let tools = b["tools"].as_array().unwrap();
    assert_eq!(
        tools.last().unwrap()["cache_control"],
        json!({"type":"ephemeral"})
    );
    assert!(
        tools[..tools.len() - 1]
            .iter()
            .all(|t| t.get("cache_control").is_none())
    );
    assert_eq!(b["system"][0]["cache_control"], json!({"type":"ephemeral"}));
    assert_eq!(
        b["messages"][2]["content"][0]["cache_control"],
        json!({"type":"ephemeral"})
    );
    assert!(
        b["messages"][0]["content"][0]
            .get("cache_control")
            .is_none(),
        "earlier turns are read from the cache, not marked"
    );
    // One turn later every earlier byte is the same but the moved mark: the prefix stays cacheable.
    let mut more = hist.as_array().unwrap().clone();
    more.push(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}));
    more.push(
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"third"}]}),
    );
    let (b2, _) = anthropic::request(&zeno_request(json!(more)), "claude-opus-5-5", 100, None);
    assert_eq!(
        (&b2["system"], &b2["tools"], &b2["messages"][0]),
        (&b["system"], &b["tools"], &b["messages"][0])
    );
    assert_eq!(
        b2["messages"][4]["content"][0]["cache_control"],
        json!({"type":"ephemeral"})
    );
    assert_eq!(b2.to_string().matches("\"cache_control\"").count(), 3);
}

#[test]
fn a_thinking_block_is_never_marked() {
    let hist = json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"q"}]},
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"a"}]},
        {"type":"reasoning","summary":[],"encrypted_content":"stratum-anthropic-thinking/1:{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"S\"}"}]);
    let (b, _) = anthropic::request(&zeno_request(hist), "claude-opus-5-5", 100, None);
    let last = b["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(last[1]["type"], "thinking");
    assert!(last[1].get("cache_control").is_none());
    assert_eq!(last[0]["cache_control"], json!({"type":"ephemeral"}));
}

#[test]
fn translated_error_state_and_frames_never_retain_arbitrary_provider_text() {
    let (_, tools) = anthropic::request(&zeno_request(json!([])), "fixture", 20, None);
    let mut translator = anthropic::Translator::new(tools);
    let out = run(
        &mut translator,
        &stream(&[json!({"type":"error","error":{
        "type":"sk-fixture-PREFIX!TAIL-SENTINEL","message":"***!TAIL-SENTINEL"}})]),
    );
    assert_eq!(translator.error.as_deref(), Some("provider stream error"));
    assert!(!serde_json::to_string(&out).unwrap().contains("SENTINEL"));
    assert!(
        !serde_json::to_string(&translator.failed("TAIL-SENTINEL", "TAIL-SENTINEL"))
            .unwrap()
            .contains("SENTINEL")
    );
}
