//! Streamed replies are assembled by the shared API layer (`stratum_api::stream`, lifted from here).

#[cfg(test)]
mod tests {
    use stratum_api::stream::{anthropic, openai};
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
