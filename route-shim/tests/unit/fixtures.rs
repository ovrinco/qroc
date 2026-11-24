use super::*;
use std::thread;
fn pair() -> (TcpStream, TcpStream) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    {
        let s = l.accept().unwrap().0;
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        (c, s)
    }
}
#[test]
fn exact_qroc_request_fake_llama_returned_model() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let upstream = thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let (_, b) = read_http(&mut s, None, 2000).ok().unwrap();
        let b: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(b["max_tokens"], 64);
        assert!(b.get("temperature").is_none());
        send(
            &mut s,
            200,
            &json!({"choices":[{"message":{"content":"fixture answer"},"finish_reason":"length"}],"model":"upstream-actual","usage":{"completion_tokens":64}}),
        );
    });
    let state = std::env::var_os("ROUTE_SHIM_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../T301W2/fixtures")
        })
        .join("rust-llama");
    fs::create_dir_all(&state).unwrap();
    let routes = std::collections::BTreeMap::from([(
        "llama:fast".into(),
        json!({"kind":"llama","base":format!("http://{addr}")}),
    )]);
    let (mut c, s) = pair();
    let worker =
        thread::spawn(move || handle(s, &routes, &state, 2000, &std::sync::Mutex::new(()), None));
    // Exactly qroc.rs's JSON construction and format! header, with fixture persona/prompt.
    let body=serde_json::to_vec(&json!({"model":"llama:fast","messages":[{"role":"system","content":"BASE_PERSONA"},{"role":"user","content":"prompt"}],"stream":false,"max_tokens":64})).unwrap();
    let host = "127.0.0.1";
    let port = c.peer_addr().unwrap().port();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    c.write_all(request.as_bytes()).unwrap();
    c.write_all(&body).unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200"));
    assert!(out.contains("Content-Length:"));
    assert!(!out.contains("Transfer-Encoding"));
    let v: Value = serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(v["model"], "upstream-actual");
    assert_eq!(v["choices"][0]["finish_reason"], "length");
    worker.join().unwrap();
    upstream.join().unwrap();
}
#[test]
fn local_unreachable_is_503() {
    let (_c, s) = pair();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    let e = local(
        &json!({"kind":"llama","base":format!("http://{addr}")}),
        &json!({}),
        &s,
        500,
    )
    .err()
    .unwrap();
    assert_eq!(e.status, 503);
}
#[test]
fn decide_design_payload_explicit_http_path() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let input = json!({"contract":{},"contract_sha":"fixture","input":"claim","source_ids":[],"tier":"cpu","deadline_ms":2000});
    let expect = input.clone();
    let worker = thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut b = vec![0; 4096];
        let n = s.read(&mut b).unwrap();
        let text = std::str::from_utf8(&b[..n]).unwrap();
        assert!(text.starts_with("POST /fixture-decide HTTP/1.1"));
        let mut bytes = b[..n].to_vec();
        while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = s.read(&mut b).unwrap();
            bytes.extend_from_slice(&b[..n]);
        }
        let p = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let len = std::str::from_utf8(&bytes[..p])
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .unwrap()
            .trim()
            .parse::<usize>()
            .unwrap();
        while bytes.len() < p + len {
            let n = s.read(&mut b).unwrap();
            bytes.extend_from_slice(&b[..n]);
        }
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes[p..]).unwrap(),
            expect
        );
        send(
            &mut s,
            200,
            &json!({"choices":["consistent"],"model":{"name":"fixture-decide"},"advisory":true}),
        );
    });
    let (_c, s) = pair();
    let result = local(
        &json!({"kind":"decide","base":format!("http://{addr}"),"path":"/fixture-decide"}),
        &json!({"messages":[{"role":"user","content":input.to_string()}]}),
        &s,
        2000,
    )
    .ok()
    .unwrap();
    assert_eq!(result.value["model"], "fixture-decide");
    worker.join().unwrap();
}
