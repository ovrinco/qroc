use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static SEQ: AtomicU64 = AtomicU64::new(0);
fn setup() -> (PathBuf, PathBuf) {
    let root = PathBuf::from(format!(
        "{}/qroc-creative-test-{}-{}",
        std::env::temp_dir().display(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("tone.wav");
    assert!(
        Command::new(TOOL)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:a",
                "pcm_s16le"
            ])
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    (root, input)
}
fn import(s: &mut CreativeStore, input: &Path) -> Value {
    s.command(
        "qroc-creative-source-import",
        &json!({"project_id":"test-project","branch_id":"branch-a","path":input}),
    )
    .unwrap()
}
fn start(s: &mut CreativeStore, a: &Value, extra: Value) -> Value {
    let mut args = json!({"adapter_id":"ffmpeg-audio-wav-v1","project_id":"test-project","branch_id":"branch-a","asset_id":a["asset_id"],"source_revision":a["revision"],"duration_seconds":0.5,"volume":0.5});
    for (k, v) in extra.as_object().unwrap() {
        args[k] = v.clone();
    }
    s.command("qroc-creative-start", &args).unwrap()
}
fn wait(s: &mut CreativeStore, o: &Value) -> Value {
    for _ in 0..200 {
        let r = s
            .command(
                "qroc-creative-status",
                &json!({"artifact_id":o["artifact_id"],"revision":o["revision"]}),
            )
            .unwrap();
        if r["status"] != "running" {
            return r;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("Timed out")
}
#[test]
fn real_conversion_lineage_restart_export_and_tamper() {
    let (root, input) = setup();
    let mut s = CreativeStore::open(&root.join("creative")).unwrap();
    let a = import(&mut s, &input);
    let job = start(&mut s, &a, json!({}));
    let first = wait(&mut s, &job);
    assert_eq!(first["status"], "succeeded");
    let out = s.path(first["file"].as_str().unwrap()).unwrap();
    let bytes = fs::read(&out).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert!(bytes.len() > 48000 && bytes.len() < 50000);
    let second = start(
        &mut s,
        &a,
        json!({"artifact_id":first["artifact_id"],"volume":1.0}),
    );
    let second = wait(&mut s, &second);
    assert_eq!(second["revision"], 2);
    assert_ne!(first["output_sha256"], second["output_sha256"]);
    let second_asset=s.command("qroc-creative-source-import",&json!({"project_id":"test-project","branch_id":"branch-a","path":input,"asset_id":a["asset_id"]})).unwrap();
    assert_eq!(second_asset["revision"], 2);
    drop(s);
    let mut s = CreativeStore::open(&root.join("creative")).unwrap();
    assert_eq!(s.state.outputs.len(), 2);
    assert_eq!(s.state.assets.len(), 2);
    let empty = s
        .command(
            "qroc-creative-list",
            &json!({"project_id":"other","branch_id":"branch-a"}),
        )
        .unwrap();
    assert_eq!(
        empty,
        json!({"schema":1,"assets":[],"outputs":[],"exports":[]})
    );
    assert!(
        s.command("qroc-creative-list", &json!({"project_id":"test-project"}))
            .is_err()
    );
    let listed = s
        .command(
            "qroc-creative-list",
            &json!({"project_id":"test-project","branch_id":"branch-a"}),
        )
        .unwrap();
    assert_eq!(listed["assets"].as_array().unwrap().len(), 2);
    assert_eq!(listed["outputs"].as_array().unwrap().len(), 2);
    let args = json!({"artifact_id":first["artifact_id"],"revision":1});
    let e = s.command("qroc-creative-export", &args).unwrap();
    let dir = Path::new(e["directory"].as_str().unwrap());
    assert_eq!(
        hash(&dir.join("artifact.wav")).unwrap(),
        first["output_sha256"]
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["publication"], "not-authorized");
    assert_eq!(manifest["output"]["source"]["sha256"], a["sha256"]);
    fs::write(&out, b"tampered").unwrap();
    assert!(
        s.command("qroc-creative-export", &args)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    drop(s);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn failure_and_cancellation_are_durable_and_not_exportable() {
    let (root, input) = setup();
    let bad = root.join("invalid.wav");
    fs::write(&bad, b"not audio").unwrap();
    let mut s = CreativeStore::open(&root.join("creative")).unwrap();
    let a = import(&mut s, &bad);
    let j = start(&mut s, &a, json!({}));
    let failed = wait(&mut s, &j);
    assert_eq!(failed["status"], "failed");
    assert!(!failed["tool_error"].as_str().unwrap().is_empty());
    let a = import(&mut s, &input);
    let j = start(&mut s, &a, json!({"duration_seconds":30}));
    let args = json!({"artifact_id":j["artifact_id"],"revision":1});
    let cancelled = s.command("qroc-creative-cancel", &args).unwrap();
    assert_eq!(cancelled["status"], "cancelled");
    assert!(s.command("qroc-creative-export", &args).is_err());
    let interrupted = start(&mut s, &a, json!({}));
    drop(s);
    let mut s = CreativeStore::open(&root.join("creative")).unwrap();
    let r = s
        .command(
            "qroc-creative-status",
            &json!({"artifact_id":interrupted["artifact_id"],"revision":1}),
        )
        .unwrap();
    assert_eq!(r["status"], "interrupted");
    assert_eq!(s.state.outputs[0]["status"], "failed");
    assert_eq!(s.state.outputs[1]["status"], "cancelled");
    drop(s);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn rejects_scope_unsupported_ranges_symlinks_and_changed_source() {
    let (root, input) = setup();
    let mut s = CreativeStore::open(&root.join("creative")).unwrap();
    let a = import(&mut s, &input);
    let args = json!({"project_id":"test-project","branch_id":"branch-a","adapter_id":"image-art","asset_id":a["asset_id"],"source_revision":1});
    assert!(
        s.command("qroc-creative-start", &args)
            .unwrap_err()
            .to_string()
            .contains("capability-unavailable")
    );
    let mut args = args;
    args["adapter_id"] = json!("ffmpeg-audio-wav-v1");
    args["duration_seconds"] = json!(31);
    assert!(s.command("qroc-creative-start", &args).is_err());
    args["duration_seconds"] = json!(1);
    args["branch_id"] = json!("other");
    assert!(
        s.command("qroc-creative-start", &args)
            .unwrap_err()
            .to_string()
            .contains("scope mismatch")
    );
    args["branch_id"] = json!("branch-a");
    let link = root.join("linked.wav");
    std::os::unix::fs::symlink(&input, &link).unwrap();
    assert!(
        s.command(
            "qroc-creative-source-import",
            &json!({"path":link,"project_id":"test-project","branch_id":"branch-a"})
        )
        .is_err()
    );
    fs::write(s.path(a["file"].as_str().unwrap()).unwrap(), b"changed").unwrap();
    assert!(
        s.command("qroc-creative-start", &args)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
    assert!(s.state.outputs.is_empty());
    drop(s);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn foundation_bridge_routes_creative_namespace() {
    let (root, input) = setup();
    let mut bridge = crate::bridge::Bridge::open(&root.join("bridge")).unwrap();
    let empty =
        bridge.reply(json!({"version":1,"id":"empty","command":"qroc-creative-list","args":{}}));
    assert_eq!(
        empty["result"],
        json!({"schema":1,"assets":[],"outputs":[],"exports":[]})
    );
    let refusal = bridge.reply(json!({"version":1,"id":"unsupported","command":"qroc-creative-unsupported-test","args":{}}));
    assert_eq!(
        refusal["error"],
        "capability-unavailable: creative command is not implemented"
    );

    let project = bridge.reply(json!({"version":1,"id":"project","command":"qroc-project-create","args":{"title":"Creative ownership fixture","brain_dump":"Prepare a local sound preview while retaining actual project and branch ownership."}}));
    assert_eq!(project["ok"], true, "{project}");
    let project_id = project["result"]["id"].as_str().unwrap();
    let branch = bridge.reply(json!({"version":1,"id":"branch","command":"qroc-branch-create","args":{"project_id":project_id,"name":"Sound preview"}}));
    assert_eq!(branch["ok"], true, "{branch}");
    let branch_id = branch["result"]["id"].as_str().unwrap();
    let r = bridge.reply(json!({"version":1,"id":"asset","command":"qroc-creative-source-import","args":{"project_id":project_id,"branch_id":branch_id,"path":input}}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["project_id"], project_id);
    assert_eq!(r["result"]["branch_id"], branch_id);
    let r = bridge
        .reply(json!({"version":1,"id":"caps","command":"qroc-creative-capabilities","args":{}}));
    assert_eq!(r["result"]["capabilities"][0]["supported"], true);

    let other_project = bridge.reply(json!({"version":1,"id":"other-project","command":"qroc-project-create","args":{"title":"Separate creative owner","brain_dump":"A separate project must not donate its branch ownership."}}));
    assert_eq!(other_project["ok"], true, "{other_project}");
    let other_id = other_project["result"]["id"].as_str().unwrap();
    let other_branch = bridge.reply(json!({"version":1,"id":"other-branch","command":"qroc-branch-create","args":{"project_id":other_id,"name":"Separate preview"}}));
    assert_eq!(other_branch["ok"], true, "{other_branch}");
    let other_branch_id = other_branch["result"]["id"].as_str().unwrap();
    for (id, requested_project, requested_branch) in [
        ("missing-project", "missing-creative-owner", branch_id),
        ("missing-branch", project_id, "missing-creative-branch"),
        ("wrong-branch-owner", project_id, other_branch_id),
    ] {
        let refusal = bridge.reply(json!({"version":1,"id":id,"command":"qroc-creative-source-import","args":{"project_id":requested_project,"branch_id":requested_branch,"path":input}}));
        assert_eq!(refusal["ok"], false, "{id}: {refusal}");
        assert!(refusal["result"].is_null(), "{id}: {refusal}");
        assert!(
            !refusal["error"].as_str().unwrap().is_empty(),
            "{id}: {refusal}"
        );
    }
    let after = bridge
        .reply(json!({"version":1,"id":"after-refusals","command":"qroc-creative-list","args":{}}));
    assert_eq!(after["ok"], true, "{after}");
    assert_eq!(after["result"]["assets"].as_array().unwrap().len(), 1);
    drop(bridge);
    fs::remove_dir_all(root).unwrap();
}
