//! Short values of secret-named environment variables (a colour such as `#999999`, a size such as
//! `1024`) are ordinary settings, not credentials: the generic mask ignores values under 8 characters,
//! as before the broker, and the legacy transport's reply is matched only against its own request
//! credentials, never against the environment. The test sets the variables on a copy of itself.
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
};
use stratum_api::{chat, keys};

const CHILD: &str = "STRATUM_ENV_MASK_CHILD";
const NAME: &str = "short_secret_named_environment_values_leave_reply_text_alone";
const HEADER_KEY: &str = "sk-fixture-legacy-SENTINEL";
const LONG_ENV: &str = "envSENTINELvalue42";

fn provider(body: &'static [u8]) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut seen = Vec::new();
            let mut b = [0; 4096];
            while let Ok(n) = s.read(&mut b) {
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&b[..n]);
                if let Some(p) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&seen[..p]).to_ascii_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if seen.len() >= p + 4 + len {
                        break;
                    }
                }
            }
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = s.write_all(body);
        }
    });
    port
}

#[test]
fn short_secret_named_environment_values_leave_reply_text_alone() {
    if std::env::var_os(CHILD).is_none() {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .env("FIXTURE_LOG_KEY", "#999999")
            .env("FIXTURE_MAX_TOKENS", "1024")
            .env("FIXTURE_LONG_TOKEN", LONG_ENV)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        let why = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{said}{why}");
        assert!(said.contains("1 passed"), "{said}");
        return;
    }
    let text = br##"{"output_text":"use the colour #999999 for the border","max":1024}"##;
    assert_eq!(
        String::from_utf8_lossy(&keys::mask(text, &[])),
        String::from_utf8_lossy(text)
    );
    // A long secret-named value is still hidden, as it always was.
    let long = format!("{{\"note\":\"{LONG_ENV}\"}}");
    assert_eq!(keys::mask(long.as_bytes(), &[]), b"{\"note\":\"***\"}");

    let body: &'static [u8] =
        br##"{"output_text":"use the colour #999999 for the border","max":1024,"echo":"sk-fixture-legacy-SENTINEL"}"##;
    let port = provider(body);
    let mut seen = Vec::new();
    let ran = chat::post(
        &format!("http://127.0.0.1:{port}/v1/chat/completions"),
        &[format!("Authorization: Bearer {HEADER_KEY}")],
        &json!({}),
        10_000,
        &|| false,
        &mut |b| {
            seen.extend_from_slice(b);
            true
        },
    );
    assert_eq!(ran.status, Some(200), "{ran:?}");
    let want =
        br##"{"output_text":"use the colour #999999 for the border","max":1024,"echo":"***"}"##;
    // The echoed request credential is hidden; the colour and the number are not touched.
    assert_eq!(
        String::from_utf8_lossy(&seen),
        String::from_utf8_lossy(want)
    );
    assert_eq!(
        String::from_utf8_lossy(&ran.body),
        String::from_utf8_lossy(want)
    );
}
