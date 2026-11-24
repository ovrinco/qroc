//! Internal recon for hardening: before the web or the person is asked, our own disk is read
//! through Lorengine (`lorengine ask`, local, no daemon, no model). Findings keep their source
//! (path and line); nothing under the person's private folder is ever kept. An unavailable
//! Lorengine is recorded and hardening goes on as before; nothing is made up in its place.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Stores read: never the voice store (Lorengine withholds it from agent callers as well).
const STORES: &str = "campaigns,decisions,namespace";
const PRIVATE: &str = "/.local/share/ovrin-private";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_EXCERPT: usize = 600;
/// Findings kept for the idea, and for each question.
pub const IDEA_FINDINGS: usize = 8;
pub const QUESTION_FINDINGS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    pub path: String,
    pub line: Option<u64>,
    pub store: String,
    pub kind: Option<String>,
    pub reference: Option<String>,
    pub standing: Option<String>,
    pub written_by: Option<String>,
    pub date: Option<String>,
    pub excerpt: String,
    pub relevance: f64,
}

impl Finding {
    /// Where it was found, as `path:line`.
    pub fn source(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path),
            None => self.path.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recon {
    /// "searched" when Lorengine answered; "unavailable" when it could not be used.
    pub status: String,
    pub queries: Vec<String>,
    pub findings: Vec<Finding>,
    pub reason: Option<String>,
}

impl Recon {
    pub fn unavailable(queries: Vec<String>, reason: String) -> Self {
        Self {
            status: "unavailable".into(),
            queries,
            findings: Vec::new(),
            reason: Some(reason),
        }
    }
}

/// The Lorengine command: `QROC_LORENGINE` when set, else `lorengine` on the path.
fn program() -> String {
    std::env::var("QROC_LORENGINE")
        .ok()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "lorengine".into())
}

fn ask_one(query: &str, limit: usize) -> Result<Vec<Finding>, String> {
    let mut child = Command::new(program())
        .args([
            "ask",
            "--json",
            "-n",
            &limit.to_string(),
            "--store",
            STORES,
            query,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Lorengine could not be started: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Lorengine did not answer within 30 seconds".into());
            }
            Err(e) => return Err(format!("Lorengine could not be read: {e}")),
        }
    };
    let mut out = String::new();
    child
        .stdout
        .take()
        .ok_or("Lorengine gave no output")?
        .read_to_string(&mut out)
        .map_err(|e| format!("Lorengine output unreadable: {e}"))?;
    if !status.success() {
        return Err(format!("Lorengine exited with {status}"));
    }
    parse(&out)
}

/// Reads `lorengine ask --json`; a hit without a path, or under the private folder, is dropped.
pub fn parse(text: &str) -> Result<Vec<Finding>, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("Lorengine reply is not JSON: {e}"))?;
    let hits = value["hits"]
        .as_array()
        .ok_or("Lorengine reply has no hits list")?;
    let opt = |h: &Value, k: &str| h[k].as_str().map(str::to_owned);
    Ok(hits
        .iter()
        .filter_map(|h| {
            let path = h["path"].as_str()?.to_owned();
            if path.trim().is_empty() || path.contains(PRIVATE) {
                return None;
            }
            let mut excerpt = h["excerpt"]
                .as_str()
                .or_else(|| h["text"].as_str())
                .unwrap_or_default()
                .trim()
                .to_owned();
            if excerpt.len() > MAX_EXCERPT {
                let mut end = MAX_EXCERPT;
                while !excerpt.is_char_boundary(end) {
                    end -= 1;
                }
                excerpt.truncate(end);
                excerpt.push('…');
            }
            Some(Finding {
                path,
                line: h["line"].as_u64(),
                store: opt(h, "store").unwrap_or_default(),
                kind: opt(h, "kind"),
                reference: opt(h, "reference"),
                standing: opt(h, "standing"),
                written_by: opt(h, "written_by"),
                date: opt(h, "date"),
                excerpt,
                relevance: h["relevance"].as_f64().unwrap_or(0.0),
            })
        })
        .collect())
}

/// Every query in turn; findings merged by source, most relevant first, at most `limit`.
pub fn search(queries: &[String], limit: usize) -> Recon {
    let queries = queries
        .iter()
        .map(|q| q.trim().to_owned())
        .filter(|q| !q.is_empty())
        .collect::<Vec<_>>();
    if queries.is_empty() {
        return Recon::unavailable(queries, "nothing to search for".into());
    }
    let mut findings: Vec<Finding> = Vec::new();
    for query in &queries {
        match ask_one(query, limit) {
            Ok(hits) => {
                for hit in hits {
                    if !findings.iter().any(|f| f.source() == hit.source()) {
                        findings.push(hit);
                    }
                }
            }
            Err(reason) => return Recon::unavailable(queries.clone(), reason),
        }
    }
    findings.sort_by(|a, b| b.relevance.total_cmp(&a.relevance));
    findings.truncate(limit);
    Recon {
        status: "searched".into(),
        queries,
        findings,
        reason: None,
    }
}

/// Queries for an idea: its opening words, short and longer.
pub fn idea_queries(idea: &str) -> Vec<String> {
    let words = idea.split_whitespace().collect::<Vec<_>>();
    let short = words.iter().take(12).copied().collect::<Vec<_>>().join(" ");
    let long = words.iter().take(40).copied().collect::<Vec<_>>().join(" ");
    if long == short {
        vec![short]
    } else {
        vec![short, long]
    }
}

/// The findings as a block of evidence with their sources, for a prompt or the hardened idea.
pub fn evidence(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            let who = match (f.written_by.as_deref(), f.reference.as_deref()) {
                (Some(w), Some(r)) => format!(" ({r}, written by {w})"),
                (Some(w), None) => format!(" (written by {w})"),
                (None, Some(r)) => format!(" ({r})"),
                (None, None) => String::new(),
            };
            format!("- [{}]{who} {}", f.source(), f.excerpt.replace('\n', " "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lines of the cited record read past its first line, for a statement that runs on.
const RECORD_LINES: usize = 40;
const MAX_RECORD_BYTES: u64 = 8 * 1024 * 1024;

/// Words only, in order: case, whitespace, punctuation and quoting do not count.
fn words(text: &str) -> String {
    text.replace("\\n", " ")
        .replace("\\\"", " ")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The cited lines of a finding's own record: its excerpt and, when the file can be read, its
/// line and the lines after it.
fn record_text(f: &Finding) -> String {
    let mut text = f.excerpt.clone();
    let readable =
        std::fs::metadata(&f.path).is_ok_and(|m| m.is_file() && m.len() <= MAX_RECORD_BYTES);
    if readable
        && !f.path.contains(PRIVATE)
        && let Ok(file) = std::fs::read_to_string(&f.path)
    {
        let first = f.line.map_or(0, |l| l.saturating_sub(1) as usize);
        for line in file.lines().skip(first).take(RECORD_LINES) {
            text.push('\n');
            text.push_str(line);
        }
    }
    text
}

/// True when what a found answer says is in its cited record: the whole statement, or each
/// part of a quotation joined by an ellipsis, word for word (case, whitespace and punctuation
/// aside). A statement of fewer than three words is never taken as found.
pub fn says_in_record(f: &Finding, says: &str) -> bool {
    let parts = says
        .split(['…'])
        .flat_map(|p| p.split("..."))
        .map(words)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if parts.iter().map(|p| p.split(' ').count()).sum::<usize>() < 3 {
        return false;
    }
    let record = format!(" {} ", words(&record_text(f)));
    parts.iter().all(|p| record.contains(&format!(" {p} ")))
}

/// The finding a source (`path` or `path:line`) names, when it is one of the recon's own.
pub fn finding_for<'a>(findings: &'a [Finding], source: &str) -> Option<&'a Finding> {
    let source = source.trim();
    findings
        .iter()
        .find(|f| f.source() == source || f.path == source)
}

/// True when `source` (`path` or `path:line`) is one of the findings' own sources.
pub fn cites(findings: &[Finding], source: &str) -> bool {
    let source = source.trim();
    findings
        .iter()
        .any(|f| f.source() == source || f.path == source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_pathless_hits_are_dropped_and_sources_kept() {
        let text = r#"{"hits":[
            {"path":"/example/operator/06_memory/a.md","line":3,"store":"campaigns","excerpt":"kept","relevance":0.5,"written_by":"him","reference":"OD-1"},
            {"path":"/example/operator/.local/share/ovrin-private/x.md","line":1,"store":"namespace","excerpt":"private","relevance":0.9},
            {"store":"decisions","excerpt":"no path","relevance":0.8}]}"#;
        let found = parse(text).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source(), "/example/operator/06_memory/a.md:3");
        assert!(
            evidence(&found)
                .contains("[/example/operator/06_memory/a.md:3] (OD-1, written by him) kept")
        );
        assert!(cites(&found, "/example/operator/06_memory/a.md:3"));
        assert!(!cites(&found, "/example/operator/06_memory/b.md:3"));
    }

    fn finding(path: &str, line: Option<u64>, excerpt: &str) -> Finding {
        Finding {
            path: path.into(),
            line,
            store: "decisions".into(),
            kind: None,
            reference: None,
            standing: None,
            written_by: None,
            date: None,
            excerpt: excerpt.into(),
            relevance: 0.5,
        }
    }

    #[test]
    fn a_found_answer_must_say_what_its_record_holds() {
        let f = finding(
            "/nonexistent/record.md",
            Some(3),
            "…the first release runs on the local decision model first; one operator runs it. The cloud…",
        );
        assert!(says_in_record(&f, "One operator  RUNS it"));
        assert!(says_in_record(
            &f,
            "\"the first release runs on the local decision model first\""
        ));
        assert!(says_in_record(
            &f,
            "the first release runs … one operator runs it"
        ));
        // The walker's case: a real source with words the record does not hold.
        assert!(!says_in_record(
            &f,
            "the record at this source, cited by the walk's stand-in"
        ));
        assert!(!says_in_record(&f, "two operators run it"));
        assert!(!says_in_record(
            &f,
            "the first release runs … in the cloud only"
        ));
        assert!(!says_in_record(&f, "runs it"), "too short to be found");
        // A word inside another word is not the word.
        assert!(!says_in_record(&f, "first release run on"));
    }

    #[test]
    fn the_cited_lines_on_disk_count_as_the_record() {
        let dir = std::env::temp_dir().join(format!("qroc-recon-says-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.md");
        std::fs::write(&path, "line one\nThe tracker stays the source of truth.\nIt is written only\nthrough the tracker command.\n").unwrap();
        let f = finding(path.to_str().unwrap(), Some(2), "The tracker stays…");
        assert!(says_in_record(
            &f,
            "it is written only through the tracker command"
        ));
        assert!(
            !says_in_record(&f, "line one is the source"),
            "a line before the cited one is not cited"
        );
    }

    #[test]
    fn unreadable_reply_is_an_error_not_a_finding() {
        assert!(parse("not json").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn idea_queries_take_the_opening_words() {
        assert_eq!(
            idea_queries("Zavent Sheets edits rows"),
            ["Zavent Sheets edits rows"]
        );
        let long = (0..50)
            .map(|n| format!("w{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        let q = idea_queries(&long);
        assert_eq!(q[0].split(' ').count(), 12);
        assert_eq!(q[1].split(' ').count(), 40);
    }
}
