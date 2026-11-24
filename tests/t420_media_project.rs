//! T420: the media project type. Every audio file here is a generated fixture tone.
use qroc_standalone::{bridge::Bridge, media, qroc::QrocStore};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn private_root(label: &str) -> PathBuf {
    let root = PathBuf::from(std::env::var_os("QROC_TEST_ROOT").expect("private QROC_TEST_ROOT"))
        .join(format!("t420-{label}-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

/// Fixture: a mono 16-bit 48 kHz sine tone written by the test itself.
fn fixture_tone(path: &Path, hz: f64, seconds: f64) {
    let rate = 48_000u32;
    let samples = (f64::from(rate) * seconds) as u32;
    let mut bytes = Vec::with_capacity(44 + samples as usize * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(samples * 2).to_le_bytes());
    for i in 0..samples {
        let v = (f64::from(i) * hz * std::f64::consts::TAU / f64::from(rate)).sin() * 8000.0;
        bytes.extend_from_slice(&(v as i16).to_le_bytes());
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn create(store: &mut QrocStore, setting: Option<&Path>) -> String {
    let made = media::command(
        store,
        setting,
        "qroc-media-create",
        &json!({"title":"Fixture track","brief":"Fixture brief","lyrics":"Fixture lyrics line"}),
    )
    .unwrap();
    assert_eq!(made["project"]["kind"], "media");
    made["project"]["id"].as_str().unwrap().to_owned()
}

#[test]
fn media_versions_record_size_and_hash_and_show_changes() {
    let root = private_root("versions");
    let store_dir = root.join("fixture-media-store");
    let (a, b) = (
        store_dir.join("take one.wav"),
        store_dir.join("take two.wav"),
    );
    fixture_tone(&a, 440.0, 1.0);
    fixture_tone(&b, 660.0, 1.5);
    let mut store = QrocStore::open(&root.join("qroc")).unwrap();
    let id = create(&mut store, Some(&store_dir));
    let before = fs::read(&a).unwrap();
    let v1 = media::command(
        &mut store,
        Some(&store_dir),
        "qroc-media-version-add",
        &json!({"project_id":id,"name":"Take one","path":a}),
    )
    .unwrap();
    assert_eq!(v1["bytes"], before.len() as u64);
    assert_eq!(v1["duration_seconds"], 1.0);
    media::command(
        &mut store,
        Some(&store_dir),
        "qroc-media-version-add",
        &json!({"project_id":id,"name":"Take two","path":b}),
    )
    .unwrap();
    // Referencing reads only: the file's bytes are as they were.
    assert_eq!(fs::read(&a).unwrap(), before);
    fixture_tone(&b, 880.0, 1.5);
    let listed = media::command(
        &mut store,
        None,
        "qroc-media-version-list",
        &json!({"project_id":id}),
    )
    .unwrap();
    let states: Vec<_> = listed["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["state"].clone())
        .collect();
    assert_eq!(states, [json!("unchanged"), json!("changed")]);
    fs::rename(&a, store_dir.join("moved.wav")).unwrap();
    let listed = media::command(
        &mut store,
        None,
        "qroc-media-version-list",
        &json!({"project_id":id}),
    )
    .unwrap();
    assert_eq!(listed["versions"][0]["state"], "missing");
}

#[test]
fn media_reference_outside_store_or_on_product_project_is_refused() {
    let root = private_root("refuse");
    let store_dir = root.join("fixture-media-store");
    let outside = root.join("elsewhere/tone.wav");
    fixture_tone(&outside, 440.0, 0.5);
    let mut store = QrocStore::open(&root.join("qroc")).unwrap();
    let id = create(&mut store, Some(&store_dir));
    let err = media::command(
        &mut store,
        Some(&store_dir),
        "qroc-media-version-add",
        &json!({"project_id":id,"name":"Out","path":outside}),
    )
    .unwrap_err();
    assert!(err.to_string().contains("media store"));
    // No setting: the store is the project folder's media/, which does not hold this file.
    assert!(
        media::command(
            &mut store,
            None,
            "qroc-media-version-add",
            &json!({"project_id":id,"name":"Out","path":outside})
        )
        .is_err()
    );
    let product = store
        .command(
            "qroc-project-create",
            &json!({"title":"Fixture product","brain_dump":"Fixture idea"}),
        )
        .unwrap();
    assert_eq!(product["kind"], "product");
    assert!(
        media::command(
            &mut store,
            None,
            "qroc-media-version-list",
            &json!({"project_id":product["id"]})
        )
        .is_err()
    );
}

#[test]
fn media_documents_are_revisioned() {
    let root = private_root("documents");
    let mut store = QrocStore::open(&root.join("qroc")).unwrap();
    let id = create(&mut store, None);
    for text in ["Fixture concept one", "Fixture concept two"] {
        media::command(
            &mut store,
            None,
            "qroc-media-document-save",
            &json!({"project_id":id,"document":"concepts","content":text}),
        )
        .unwrap();
    }
    let got = media::command(
        &mut store,
        None,
        "qroc-media-document-get",
        &json!({"project_id":id,"document":"concepts"}),
    )
    .unwrap();
    assert_eq!(got["document"]["revision"], 2);
    assert_eq!(got["revisions"], json!([1, 2]));
    let first = media::command(
        &mut store,
        None,
        "qroc-media-document-get",
        &json!({"project_id":id,"document":"concepts","revision":1}),
    )
    .unwrap();
    assert_eq!(first["document"]["content"], "Fixture concept one");
}

fn call(bridge: &mut Bridge, n: &mut u32, command: &str, args: Value) -> Value {
    *n += 1;
    let reply =
        bridge.reply(json!({"version":1,"id":format!("r{n}"),"command":command,"args":args}));
    assert_eq!(reply["ok"], true, "{command}: {reply}");
    reply["result"].clone()
}

#[test]
fn media_folder_keeps_the_persons_files_and_trim_reads_a_version_in_place() {
    let root = private_root("folder");
    let mut bridge = Bridge::open(&root).unwrap();
    let n = &mut 0;
    let made = call(
        &mut bridge,
        n,
        "qroc-media-create",
        json!({"title":"Fixture piece","brief":"Fixture brief","lyrics":"Fixture lyrics","concepts":"Fixture concept"}),
    );
    let id = made["project"]["id"].as_str().unwrap().to_owned();
    let folder = PathBuf::from(
        call(
            &mut bridge,
            n,
            "qroc-project-open-folder",
            json!({"project_id":id}),
        )["folder"]
            .as_str()
            .unwrap(),
    );
    // Default media store: the project folder's media/.
    let tone = folder.join("media/fixture tone.wav");
    fixture_tone(&tone, 440.0, 2.0);
    let before = fs::read(&tone).unwrap();
    call(
        &mut bridge,
        n,
        "qroc-media-version-add",
        json!({"project_id":id,"name":"First mix","path":tone}),
    );
    call(
        &mut bridge,
        n,
        "qroc-media-document-save",
        json!({"project_id":id,"document":"brief","content":"Fixture brief, second"}),
    );
    let again = PathBuf::from(
        call(
            &mut bridge,
            n,
            "qroc-project-open-folder",
            json!({"project_id":id}),
        )["folder"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(again, folder);
    assert_eq!(
        fs::read(&tone).unwrap(),
        before,
        "the person's file stays where it was, unchanged"
    );
    for name in [
        "Brief.md",
        "Lyrics.md",
        "Concepts.md",
        "Versions.md",
        "revisions/Brief - revision 1.md",
    ] {
        assert!(folder.join("media").join(name).is_file(), "{name}");
    }
    let versions = fs::read_to_string(folder.join("media/Versions.md")).unwrap();
    assert!(versions.contains("First mix") && versions.contains("unchanged"));
    assert!(
        !versions.contains(root.to_str().unwrap()),
        "names, not paths"
    );
    // A file of the person's named like a project text is never overwritten.
    fs::remove_file(folder.join("media/.qroc-written")).unwrap();
    *n += 1;
    let refused = bridge.reply(json!({"version":1,"id":format!("r{n}"),"command":"qroc-project-open-folder","args":{"project_id":id}}));
    assert_eq!(refused["ok"], false);
    fs::write(
        folder.join("media/.qroc-written"),
        "Brief.md\nConcepts.md\nLyrics.md\nVersions.md\nrevisions/Brief - revision 1.md\n",
    )
    .unwrap();
    // The existing trim works on the version's file, read in place.
    if !Path::new("/usr/bin/ffmpeg").exists() {
        return;
    }
    let started = call(
        &mut bridge,
        n,
        "qroc-media-version-trim",
        json!({"project_id":id,"version":1,"duration_seconds":0.5}),
    );
    let (artifact, revision) = (started["artifact_id"].clone(), started["revision"].clone());
    let mut status = started;
    for _ in 0..200 {
        if status["status"] != "running" {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        status = call(
            &mut bridge,
            n,
            "qroc-creative-status",
            json!({"artifact_id":artifact,"revision":revision}),
        );
    }
    assert_eq!(status["status"], "succeeded", "{status}");
    assert_eq!(status["source"]["reference"], json!(tone));
    assert_eq!(fs::read(&tone).unwrap(), before);
    fixture_tone(&tone, 550.0, 2.0);
    *n += 1;
    let changed = bridge.reply(json!({"version":1,"id":format!("r{n}"),"command":"qroc-media-version-trim","args":{"project_id":id,"version":1}}));
    assert_eq!(changed["ok"], false, "a changed version is not trimmed");
}

#[test]
fn media_folder_refresh_never_moves_the_persons_files() {
    use std::os::unix::fs::MetadataExt;
    let root = private_root("nomove");
    let mut bridge = Bridge::open(&root).unwrap();
    let n = &mut 0;
    let made = call(
        &mut bridge,
        n,
        "qroc-media-create",
        json!({"title":"Fixture still","brief":"Fixture brief"}),
    );
    let id = made["project"]["id"].as_str().unwrap().to_owned();
    let folder = PathBuf::from(
        call(
            &mut bridge,
            n,
            "qroc-project-open-folder",
            json!({"project_id":id}),
        )["folder"]
            .as_str()
            .unwrap(),
    );
    let media = folder.join("media");
    let tone = media.join("fixture keep.wav");
    fixture_tone(&tone, 330.0, 0.5);
    let (dir_before, folder_before) = (
        fs::metadata(&media).unwrap().ino(),
        fs::metadata(&folder).unwrap().ino(),
    );
    let before = fs::metadata(&tone).unwrap();
    let bytes = fs::read(&tone).unwrap();
    for text in ["Fixture brief, second", "Fixture brief, third"] {
        call(
            &mut bridge,
            n,
            "qroc-media-document-save",
            json!({"project_id":id,"document":"brief","content":text}),
        );
        call(
            &mut bridge,
            n,
            "qroc-project-open-folder",
            json!({"project_id":id}),
        );
    }
    let after = fs::metadata(&tone).unwrap();
    assert_eq!(after.ino(), before.ino());
    assert_eq!(after.mtime(), before.mtime());
    assert_eq!(after.mtime_nsec(), before.mtime_nsec());
    assert_eq!(fs::read(&tone).unwrap(), bytes);
    assert_eq!(fs::metadata(&media).unwrap().ino(), dir_before);
    assert_eq!(fs::metadata(&folder).unwrap().ino(), folder_before);
    assert_eq!(
        fs::read_to_string(media.join("Brief.md")).unwrap(),
        "Fixture brief, third"
    );
    assert!(media.join("revisions/Brief - revision 2.md").is_file());
}
