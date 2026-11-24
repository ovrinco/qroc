use qroc_standalone::{
    bridge::{Bridge, MAX_FRAME},
    qroc::QrocStore,
};
use serde_json::{Value, json};
use std::{fs, io::Cursor, path::PathBuf};
fn root(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(format!("scratch-test-{}-{name}", std::process::id()));
    fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn transport_preserves_store_payload_and_errors() {
    let p = root("payload");
    let mut bridge = Bridge::open(&p.join("bridge")).unwrap();
    let mut store = QrocStore::open(&p.join("direct")).unwrap();
    for (i, command) in ["qroc-project-list", "qroc-document-save", "qroc-not-real"]
        .iter()
        .enumerate()
    {
        let reply =
            bridge.reply(json!({"version":1,"id":i.to_string(),"command":command,"args":{}}));
        match store.command(command, &json!({})) {
            Ok(v) => {
                assert_eq!(reply["ok"], true);
                assert_eq!(reply["result"], v)
            }
            Err(e) => {
                assert_eq!(reply["ok"], false);
                assert_eq!(reply["error"], e.to_string())
            }
        }
    }
    let req = json!({"version":1,"id":"dup","command":"qroc-project-create","args":{"title":"Ideas","brain_dump":"Retain pantry source evidence"}});
    assert_eq!(bridge.reply(req.clone())["ok"], true);
    assert_eq!(bridge.reply(req)["error"], "Duplicate request ID");
    let projects =
        bridge.reply(json!({"version":1,"id":"list","command":"qroc-project-list","args":{}}));
    assert_eq!(projects["result"]["projects"].as_array().unwrap().len(), 1);
    // Integration standing is adapter-owned; reserved prefixes are not all unavailable.
    let capabilities=bridge.reply(json!({"version":1,"id":"extension-capabilities","command":"bridge-capabilities","args":{}}));
    assert_eq!(capabilities["version"], 1);
    assert_eq!(capabilities["id"], "extension-capabilities");
    assert_eq!(capabilities["ok"], true);
    assert!(capabilities["error"].is_null());
    for extension in ["scope", "registry", "creative"] {
        assert!(
            capabilities["result"][extension].is_boolean(),
            "Missing capability standing for {extension}"
        );
    }
    // Authoring v1 reserves scope/registry discovery under -get/-catalog, not -list.
    for extension in ["scope", "registry"] {
        let id = format!("unsupported-{extension}-list");
        let r = bridge.reply(
            json!({"version":1,"id":id,"command":format!("qroc-{extension}-list"),"args":{}}),
        );
        assert_eq!(r["version"], 1);
        assert_eq!(r["id"], id);
        assert_eq!(r["ok"], false);
        assert!(r["result"].is_null());
        assert!(!r["error"].as_str().unwrap().is_empty());
    }
    // Creative list is supported even when the conversion tool is unavailable.
    // Match bridge discovery to the real adapter, not a hardcoded true/false value.
    let adapter =
        qroc_standalone::creative::CreativeStore::open(&p.join("creative-capability-probe"))
            .unwrap();
    assert_eq!(capabilities["result"]["creative"], adapter.available());
    let creative = bridge
        .reply(json!({"version":1,"id":"creative-empty","command":"qroc-creative-list","args":{}}));
    assert_eq!(
        creative,
        json!({"version":1,"id":"creative-empty","ok":true,"result":{"schema":1,"assets":[],"outputs":[],"exports":[]},"error":null})
    );
    let unsupported=bridge.reply(json!({"version":1,"id":"creative-refusal","command":"qroc-creative-unsupported-test","args":{}}));
    assert_eq!(
        unsupported,
        json!({"version":1,"id":"creative-refusal","ok":false,"result":null,"error":"capability-unavailable: creative command is not implemented"})
    );
    assert!(Bridge::open(&p.join("bridge")).is_err());
    drop(bridge);
    assert!(Bridge::open(&p.join("bridge")).is_ok());
    fs::remove_dir_all(p).unwrap();
}
#[test]
fn framing_recovers_bad_json_but_closes_oversize() {
    let p = root("frames");
    let mut b = Bridge::open(&p).unwrap();
    let mut out = Vec::new();
    b.serve(
        Cursor::new(
            b"broken\n{\"version\":1,\"id\":\"next\",\"command\":\"phod-list\",\"args\":{}}\n",
        ),
        &mut out,
    )
    .unwrap();
    let replies: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[1]["ok"], false);
    assert_eq!(replies[2]["id"], "next");
    assert_eq!(replies[2]["ok"], true);
    let mut out = Vec::new();
    b.serve(Cursor::new(vec![b'x'; MAX_FRAME + 1]), &mut out)
        .unwrap();
    assert!(String::from_utf8(out).unwrap().contains("Frame exceeds"));
    drop(b);
    fs::remove_dir_all(p).unwrap();
}
#[test]
fn roots_fail_closed() {
    assert!(Bridge::open(std::path::Path::new("relative")).is_err());
    let p = root("links");
    std::os::unix::fs::symlink(&p, p.join("link")).unwrap();
    assert!(Bridge::open(&p.join("link")).is_err());
    fs::remove_dir_all(p).unwrap();
}
