//! The image route through the layer's service and command line, on a stand-in (no network, no key of
//! his): a generation and an edit with reference images are reserved before sending at the most they
//! can cost, sent with the route's model, settled on the reply's usage and logged under the tool's cap;
//! a call the cap cannot hold, an automatic size or quality, a streamed call and a tool with no cap are
//! refused unsent; a provider refusal costs nothing; a reply without usage stays unknown and stops the
//! tool until reconciled; `stratum-api image` writes the images to files and prints the receipt.
mod fake;
use fake::{Fake, Root, fake, root};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
};

fn post(port: u16, path: &str, body: &Value) -> (u16, Value) {
    let b = body.to_string();
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.write_all(format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{b}", b.len()).as_bytes()).unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    (
        status,
        serde_json::from_str(out.split("\r\n\r\n").nth(1).unwrap()).unwrap(),
    )
}

/// A throwaway layer with an image model priced and routed, and a `vasm` cap of 1 USD.
fn image_root(tag: &str, f: &Fake) -> Root {
    let r = root(tag, f);
    let mut prices = fake::prices();
    prices["models"]["openai:fake-image"] = json!({"provider":"openai","model":"fake-image","kind":"image","input":"5.00","cached_input":"1.25","image_input":"8.00","cached_image_input":"2.00","output":"30.00","image_output_base":{"low":16,"medium":24,"high":48,"xhigh":64,"max":96},"input_tokens_per_image_bound":1000,"source_url":"fixture","read_on":"2026-10-30"});
    std::fs::write(r.config().join("prices.json"), prices.to_string()).unwrap();
    let mut routes: Value =
        serde_json::from_slice(&std::fs::read(r.config().join("routes.json")).unwrap()).unwrap();
    routes["routes"]["images:openai"] =
        json!({"provider":"openai","model":"fake-image","base":f.base(),"max_output_tokens":40000});
    std::fs::write(r.config().join("routes.json"), routes.to_string()).unwrap();
    r.caps(json!({"qroc":{"total":"1"},"vasm":{"total":"1"}}));
    r
}

fn cli(r: &Root) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_stratum-api"));
    c.env("STRATUM_API_HOME", r.home())
        .env("STRATUM_API_CONFIG", r.config())
        .env_remove("STRATUM_API_ALLOW_ENV_KEYS");
    c
}

fn serve(r: &Root) -> (Child, u16) {
    let mut child = cli(r)
        .args(["serve", "--listen", "127.0.0.1:0", "--idle-secs", "30"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let (mut line, mut b) = (String::new(), [0u8; 1]);
    while out.read(&mut b).unwrap() == 1 && b[0] != b'\n' {
        line.push(b[0] as char);
    }
    (
        child,
        line.rsplit(':').next().unwrap().trim().parse().unwrap(),
    )
}

const GEN: &str = "/tool/vasm/v1/images/generations";
const EDIT: &str = "/tool/vasm/v1/images/edits";

#[test]
fn generation_and_edit_reserved_sent_settled_and_logged() {
    let f = fake();
    let r = image_root("images", &f);
    let (mut child, port) = serve(&r);
    let prompt = "a face";
    let (st, reply) = post(
        port,
        GEN,
        &json!({"prompt":prompt,"size":"1024x640","quality":"low","n":2}),
    );
    assert_eq!(st, 200, "{reply}");
    // Reserved: (512 framing + 6 prompt bytes) x 8 + 2 x (107 + 16) image tokens x 30 (nano per token).
    assert_eq!(reply["reservation"], "0.011524000");
    // Settled: 20 text tokens x 5 + 214 image tokens x 30.
    assert_eq!(reply["charge"]["amount"], "0.006520000");
    assert!(
        reply["charge"]["basis"]
            .as_str()
            .unwrap()
            .contains("openai:fake-image")
    );
    assert_eq!(reply["data"].as_array().unwrap().len(), 2);
    assert_eq!(reply["data"][0]["b64_json"], fake::PNG_B64);
    assert_eq!(reply["route"], "images:openai");
    let sent = f.seen.lock().unwrap()[0].clone();
    assert_eq!(sent.0, "/v1/images/generations");
    assert!(sent.1.contains("Authorization: Bearer"));
    assert_eq!(sent.2["model"], "fake-image", "the route's model is sent");
    assert_eq!(sent.2["size"], "1024x640");
    // An edit with two reference images (data URLs), named by the route's model.
    let png = format!("data:image/png;base64,{}", fake::PNG_B64);
    let (st, reply) = post(
        port,
        EDIT,
        &json!({"model":"fake-image","prompt":"same face, smiling","size":"1024x1024","quality":"low","images":[{"image_url":png},{"image_url":png}]}),
    );
    assert_eq!(st, 200, "{reply}");
    // Reserved: (512 + 18 + 2 x 1000) x 8 + (196 + 16) x 30.
    assert_eq!(reply["reservation"], "0.026600000");
    // Settled: 20 x 5 + 200 x 8 + 107 x 30.
    assert_eq!(reply["charge"]["amount"], "0.004910000");
    let sent = f.seen.lock().unwrap()[1].clone();
    assert_eq!(sent.0, "/v1/images/edits");
    assert_eq!(sent.2["images"].as_array().unwrap().len(), 2);
    let spend = cli(&r).args(["spend", "--json"]).output().unwrap();
    let spend: Value = serde_json::from_slice(&spend.stdout).unwrap();
    let text = spend.to_string();
    assert!(
        text.contains("vasm") && text.contains("0.011430000"),
        "{text}"
    );
    let _ = child.kill();
    let _ = child.wait();
    let ledger = r.ledger();
    let lines: Vec<(&str, &str)> = ledger
        .iter()
        .map(|l| {
            (
                l["decision"].as_str().unwrap(),
                l["route"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            ("admit", "images:openai"),
            ("settle", "images:openai"),
            ("admit", "images:openai"),
            ("settle", "images:openai")
        ]
    );
    assert!(
        ledger
            .iter()
            .all(|l| l["tool"] == "vasm" && l["model"] == "fake-image")
    );
    assert_eq!(ledger[1]["generation_id"], "image-created-1793000000");
    assert!(
        !std::fs::read_to_string(r.home().join("ledger.jsonl"))
            .unwrap()
            .contains(fake::PNG_B64),
        "no image in the ledger"
    );
    let files = fake::no_key_anywhere(&r.dir);
    println!(
        "images: generation reserved 0.011524 settled 0.00652, edit with 2 references reserved 0.0266 settled 0.00491, ledger admit/settle x2 under vasm; no key in {files} files"
    );
}

#[test]
fn over_cap_unbounded_streamed_and_uncapped_calls_refused_unsent() {
    let f = fake();
    let r = image_root("images-refuse", &f);
    let (mut child, port) = serve(&r);
    let ok = json!({"prompt":"a face","size":"1024x1024","quality":"low"});
    for (path, body, status, words) in [
        (
            GEN,
            json!({"prompt":"a face","size":"auto","quality":"low"}),
            400,
            "name the size",
        ),
        (
            GEN,
            json!({"prompt":"a face","size":"1024x1024","quality":"auto"}),
            400,
            "name the quality",
        ),
        (
            GEN,
            json!({"prompt":"a face","size":"1024x1024","quality":"low","stream":true}),
            400,
            "not offered",
        ),
        (
            GEN,
            json!({"prompt":"a face","size":"1000x1000","quality":"low"}),
            400,
            "multiples of 16",
        ),
        (
            GEN,
            json!({"prompt":"a face","size":"1024x1024","quality":"ultra"}),
            400,
            "is not one of",
        ),
        (
            GEN,
            json!({"prompt":"a face","size":"3840x2160","quality":"max","n":10}),
            400,
            "allows at most",
        ),
        (
            EDIT,
            json!({"prompt":"a face","size":"1024x1024","quality":"low"}),
            400,
            "reference image",
        ),
        (
            GEN,
            json!({"model":"gpt-other","prompt":"a face","size":"1024x1024","quality":"low"}),
            400,
            "not an image route",
        ),
        (
            "/tool/nocap/v1/images/generations",
            ok.clone(),
            503,
            "has no cap",
        ),
    ] {
        let (st, reply) = post(port, path, &body);
        assert_eq!(st, status, "{body} -> {reply}");
        assert!(reply["error"].as_str().unwrap().contains(words), "{reply}");
    }
    // A cap the reservation would pass: 1024x1024 high reserves 518 x 8 + (1756 + 16) x 30 = 0.057304.
    r.caps(json!({"vasm":{"total":"0.01"}}));
    let (st, reply) = post(
        port,
        GEN,
        &json!({"prompt":"a face","size":"1024x1024","quality":"high"}),
    );
    assert_eq!(st, 429, "{reply}");
    assert_eq!(reply["error"], "tool cap reached for vasm");
    assert_eq!(reply["detail"]["cap"], "0.010000000");
    assert_eq!(f.hits(), 0, "nothing refused ever reached the provider");
    // A low call reserves 518 x 8 + 212 x 30 = 0.010504, also past 0.01.
    let (st, _) = post(port, GEN, &ok);
    assert_eq!(st, 429, "the low call's reservation also passes 0.01");
    r.caps(json!({"vasm":{"total":"0.012"}}));
    let (st, reply) = post(port, GEN, &ok);
    assert_eq!(st, 200, "{reply}");
    assert_eq!(f.hits(), 1);
    // Settled 0.00331; a second call would bring it to 0.013814, past 0.012: refused unsent.
    let (st, reply) = post(port, GEN, &ok);
    assert_eq!(st, 429, "{reply}");
    assert_eq!(f.hits(), 1);
    let _ = child.kill();
    let _ = child.wait();
    let refusals = r
        .ledger()
        .iter()
        .filter(|l| l["decision"] == "refuse")
        .count();
    assert_eq!(
        refusals, 4,
        "nocap, two over the cap, one over after a settlement"
    );
    println!(
        "images: auto size/quality, stream, bad size/quality, oversize call, edit without references, unknown route refused unsent; over-cap refused unsent (hits 1 of 13)"
    );
}

#[test]
fn provider_refusal_is_free_and_missing_usage_stops_the_tool() {
    let f = fake();
    let r = image_root("images-fail", &f);
    let (mut child, port) = serve(&r);
    let ok = json!({"prompt":"a face","size":"1024x1024","quality":"low"});
    f.set("moderation");
    let (st, reply) = post(port, GEN, &ok);
    assert_eq!(st, 502, "{reply}");
    assert_eq!(reply["charge"]["amount"], "0.000000000");
    assert_eq!(
        reply["detail"]["provider_message"],
        "the provider call failed"
    );
    f.set("no-usage");
    let (st, reply) = post(port, GEN, &ok);
    assert_eq!(st, 200, "the images are delivered: {reply}");
    assert_eq!(reply["charge"]["amount"], "unknown");
    f.set("ok");
    let (st, reply) = post(port, GEN, &ok);
    assert_eq!(st, 429, "{reply}");
    assert!(reply["error"].as_str().unwrap().contains("unresolved"));
    assert_eq!(f.hits(), 2);
    let _ = child.kill();
    let _ = child.wait();
    println!(
        "images: provider refusal settled at 0 with its words; reply without usage held unknown and stopped the tool"
    );
}

#[test]
fn command_line_writes_images_and_prints_the_receipt() {
    let f = fake();
    let r = image_root("images-cli", &f);
    let out = r.dir.join("out");
    let prompt = r.dir.join("prompt.txt");
    std::fs::write(&prompt, "a face, front view\n").unwrap();
    let run = cli(&r)
        .args(["image", "--tool", "vasm", "--prompt-file"])
        .arg(&prompt)
        .args([
            "--size",
            "1024x1024",
            "--quality",
            "low",
            "--n",
            "2",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "{text} {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(text.starts_with("Receipt "), "{text}");
    assert!(text.contains("charged 0.00652 USD"), "{text}");
    let written: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().collect();
    assert_eq!(written.len(), 2);
    for w in &written {
        assert_eq!(std::fs::read(w.path()).unwrap(), b"\x89PNG\r\n\x1a\n");
    }
    assert_eq!(f.seen.lock().unwrap()[0].2["prompt"], "a face, front view");
    // An edit from a reference image file, the receipt as JSON.
    let reference = r.dir.join("ref.png");
    std::fs::write(&reference, b"\x89PNG\r\n\x1a\n").unwrap();
    let run = cli(&r)
        .args([
            "image",
            "--tool",
            "vasm",
            "--prompt",
            "same face",
            "--size",
            "1024x1024",
            "--quality",
            "low",
            "--json",
            "--image",
        ])
        .arg(&reference)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let v: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(v["files"].as_array().unwrap().len(), 1);
    assert_eq!(v["usage"]["input_tokens_details"]["image_tokens"], 100);
    let sent = f.seen.lock().unwrap()[1].clone();
    assert_eq!(sent.0, "/v1/images/edits");
    assert_eq!(
        sent.2["images"][0]["image_url"],
        format!("data:image/png;base64,{}", fake::PNG_B64)
    );
    // Over the cap: refused unsent, in plain words, nonzero exit.
    r.caps(json!({"vasm":{"total":"0.001"}}));
    let run = cli(&r)
        .args([
            "image",
            "--tool",
            "vasm",
            "--prompt",
            "x",
            "--size",
            "1024x1024",
            "--quality",
            "low",
            "--out",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert!(!run.status.success());
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(
        err.contains("tool cap reached for vasm; nothing was sent"),
        "{err}"
    );
    assert_eq!(f.hits(), 2);
    let files = fake::no_key_anywhere(&r.dir);
    println!(
        "images: command line wrote 2 + 1 images with receipts; over-cap refused unsent with plain words; no key in {files} files"
    );
}
