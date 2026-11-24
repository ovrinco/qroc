//! Test fixture, not a judgment: a passed hardening record for every project of a store that has
//! none, so tests of other features can write documents behind the hardening gate.
#![allow(dead_code)]
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn pass_hardening(qroc_root: &Path) {
    fs::create_dir_all(qroc_root.join("hardening")).unwrap();
    for entry in fs::read_dir(qroc_root.join("projects")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let project: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let id = project["id"].as_str().unwrap();
        let record = qroc_root.join("hardening").join(format!("{id}.json"));
        if record.exists() {
            continue;
        }
        let idea = project["brain_dump"].as_str().unwrap_or("");
        let round = serde_json::json!({"round":1,"at":0,"idea_text":idea,
            "idea_sha256":format!("{:x}", Sha256::digest(idea.as_bytes())),
            "score":{"total":100,"idea":100,"config":0,"missing":[]},"verdict":"pass","sent_back":false,
            "questions":null,"answers":null,"judgment":{"status":"test fixture","answers":[],"receipt":null}});
        let value = serde_json::json!({"schema":"qroc-hardening/1","project":id,"rounds":[round],"route":null});
        fs::write(record, value.to_string()).unwrap();
    }
}
