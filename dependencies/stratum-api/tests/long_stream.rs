//! A long streamed reply (400 deltas over about a second) arrives whole and is settled exactly, well
//! inside the call's time limit.
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};
use stratum_api::{Billing, chat};

#[test]
fn long_stream_arrives_whole_in_time() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut b = [0; 65536];
        let mut got = Vec::new();
        while !got.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = s.read(&mut b).unwrap();
            got.extend_from_slice(&b[..n]);
        }
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
        let mut ev = vec![r#"data: {"id":"c1","model":"m","service_tier":"default","choices":[{"delta":{"content":""}}]}"#.to_string()];
        for i in 0..400 {
            ev.push(format!(
                r#"data: {{"id":"c1","choices":[{{"delta":{{"content":"piece {i}; "}}}}]}}"#
            ));
        }
        ev.push(r#"data: {"id":"c1","choices":[{"delta":{},"finish_reason":"stop"}]}"#.into());
        ev.push(
            r#"data: {"id":"c1","choices":[],"usage":{"prompt_tokens":5,"completion_tokens":9}}"#
                .into(),
        );
        ev.push("data: [DONE]".into());
        for e in ev {
            let e = format!("{e}\n\n");
            s.write_all(format!("{:x}\r\n{e}\r\n", e.len()).as_bytes())
                .unwrap();
            s.flush().unwrap();
            thread::sleep(Duration::from_micros(1500));
        }
        s.write_all(b"0\r\n\r\n").unwrap();
    });
    let base = format!("http://127.0.0.1:{port}");
    let t = Instant::now();
    let mut n = 0usize;
    // A client still waiting on its reply, watched for going away as a service does.
    let lc = TcpListener::bind("127.0.0.1:0").unwrap();
    let _waiting = std::net::TcpStream::connect(lc.local_addr().unwrap()).unwrap();
    let (client, _) = lc.accept().unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let ran = chat::post(
        &format!("{base}/v1/chat/completions"),
        &[],
        &serde_json::json!({"stream":true}),
        3000,
        &|| {
            // As route-shim checks: a peek that waits out the client's 50 ms read timeout.
            let mut b = [0; 1];
            matches!(client.peek(&mut b), Ok(0))
        },
        &mut |c| {
            n += c.len();
            true
        },
    );
    let ms = t.elapsed().as_millis();
    assert_eq!(ran.reason, "", "{ran:?}");
    assert_eq!(ran.status, Some(200));
    assert_eq!(n, ran.body.len(), "every byte handed on as it arrived");
    let a = stratum_api::stream::openai(&String::from_utf8_lossy(&ran.body));
    assert!(a.finished());
    assert!(matches!(chat::usage(a.reply), Billing::Usage { .. }));
    assert!(ms < 2500, "took {ms} ms");
    println!("long stream: 400 deltas whole in {ms} ms");
}
