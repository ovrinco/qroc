//! Anthropic's Messages API is spoken by the shared API layer (`stratum_api::anthropic`, lifted from here).
#[cfg(test)]
pub use stratum_api::anthropic::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
