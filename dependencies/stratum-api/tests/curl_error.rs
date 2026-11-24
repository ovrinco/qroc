//! A provider that resets the connection mid-upload (as the live image calls of 30 Oct saw, curl exit
//! 55/56): the failure is kept unknown and reserved, and its reason uses a fixed numeric classification.
//! Raw subprocess stderr is never captured or preprocessed.
mod fake;
use serde_json::json;
use std::{io::Read, net::TcpListener, thread};
use stratum_api::{Ask, chat, money};

/// Reads the request head only, then closes with the body unread: the kernel answers with a reset.
fn resetting() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut b = [0; 1024];
            let _ = s.read(&mut b);
            drop(s);
        }
    });
    port
}

#[test]
fn reset_mid_upload_uses_only_fixed_numeric_diagnostics() {
    let port = resetting();
    let base = format!("http://127.0.0.1:{port}");
    let big = json!({"model":"m","input":"x".repeat(4 * 1024 * 1024)});
    let ran = chat::post(
        &format!("{base}/v1/responses"),
        &["Authorization: Bearer sk-fixtureSENTINEL-curl-0123456789".into()],
        &big,
        10_000,
        &|| false,
        &mut |_| true,
    );
    assert_eq!(ran.reason, "nonzero exit", "{ran:?}");
    assert!(matches!(ran.exit, Some(52 | 55 | 56)), "{ran:?}");
    assert_eq!(
        ran.curl_error,
        format!("transport failure (curl exit {})", ran.exit.unwrap())
    );
    assert!(ran.said().starts_with("; curl said: "));
    assert!(!ran.curl_error.contains("SENTINEL"));

    // The same through the guard: settled unknown, reserved, and the basis names curl's words.
    let f = fake::fake();
    let r = fake::root("curl-error", &f);
    let mut routes: serde_json::Value =
        serde_json::from_slice(&std::fs::read(r.config().join("routes.json")).unwrap()).unwrap();
    routes["routes"]["openai:fake-gpt"]["base"] = json!(base);
    std::fs::write(r.config().join("routes.json"), routes.to_string()).unwrap();
    let layer = r.layer();
    let route = layer.route("openai:fake-gpt").unwrap();
    let messages = json!([{"role":"user","content":"y".repeat(2 * 1024 * 1024)}]);
    let ask = Ask {
        tool: "qroc",
        id: None,
        route: "openai:fake-gpt",
        provider: "openai",
        model: "fake-gpt",
        input_bound: money::input_bound(&messages),
        max_output: 100,
        fingerprint: "",
        inner: None,
    };
    r.caps(json!({"qroc":{"total":"100"}}));
    let a = layer.admit(&ask).unwrap();
    let call = chat::Call {
        id: &a.id,
        provider: "openai",
        base: &route.base,
        model: "fake-gpt",
        effort: None,
        messages: &messages,
        max_output: 100,
        timeout_ms: 10_000,
        keep_dir: None,
    };
    let out = chat::complete(&layer, &call, &|| false);
    let s = layer.settle(&a, out.billing);
    assert_eq!(s.charge, None);
    assert!(s.basis.contains("curl said: "), "{}", s.basis);
    assert_eq!(s.tool.unresolved, 1);
    let line = r
        .ledger()
        .into_iter()
        .find(|l| l["decision"] == "settle")
        .unwrap();
    assert!(line["basis"].as_str().unwrap().contains("curl said: "));
    assert!(fake::no_key_anywhere(&r.dir) > 0);
    println!("reset mid-upload: exit {:?}, basis: {}", ran.exit, s.basis);
}
