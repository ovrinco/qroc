//! Server-sent events read incrementally, as bytes arrive: each complete event (its name and its `data:`
//! lines joined by newlines) is returned once its blank line has come; comments and keep-alives are skipped.

#[derive(Debug, Default)]
pub struct Parser {
    buf: Vec<u8>,
    name: String,
    data: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub data: String,
}

impl Parser {
    /// Feed bytes; returns the events completed by them.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(at) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=at).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
            let line = line.strip_suffix('\r').unwrap_or(&line);
            if line.is_empty() {
                if !self.data.is_empty() {
                    out.push(Event {
                        name: std::mem::take(&mut self.name),
                        data: self.data.join("\n"),
                    });
                }
                self.name.clear();
                self.data.clear();
            } else if let Some(v) = line.strip_prefix("data:") {
                self.data.push(v.strip_prefix(' ').unwrap_or(v).to_owned());
            } else if let Some(v) = line.strip_prefix("event:") {
                self.name = v.trim().to_owned();
            }
        }
        out
    }

    /// At the end of the stream: an event whose blank line never came (a cut stream). The callers parse
    /// its data and so accept it only if it is whole.
    pub fn finish(&mut self) -> Option<Event> {
        let tail = std::mem::take(&mut self.buf);
        if !tail.is_empty() {
            self.feed(&tail);
            self.feed(b"\n");
        }
        (!self.data.is_empty()).then(|| Event {
            name: std::mem::take(&mut self.name),
            data: self.data.join("\n"),
        })
    }
}

/// One event in the wire form Zeno reads.
pub fn frame(kind: &str, data: &serde_json::Value) -> String {
    format!("event: {kind}\ndata: {data}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn events_split_across_reads() {
        let mut p = Parser::default();
        assert!(p.feed(b"event: a\r\ndata: {\"x\"").is_empty());
        let e = p.feed(b":1}\r\n\r\n: keep-alive\n\ndata: two\ndata: lines\n\n");
        assert_eq!(
            e,
            vec![
                Event {
                    name: "a".into(),
                    data: "{\"x\":1}".into()
                },
                Event {
                    name: "".into(),
                    data: "two\nlines".into()
                },
            ]
        );
        p.feed(b"data: cut");
        assert_eq!(p.finish().unwrap().data, "cut");
    }
}
