#[allow(dead_code)]
#[path = "../src/codex.rs"]
mod codex;
#[test]
fn text_images_are_attached_and_invalid_images_fail() {
    let root = std::path::PathBuf::from("/example/operator/.cache/qroc-gate-recovery/T03/image-test")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    let body = serde_json::json!({"messages":[{"role":"user","content":[{"type":"text","text":"Describe these frames"},{"type":"image_url","image_url":{"url":"data:image/png;base64,iVBORw0KGgo="}}]}]});
    let (text, images) = codex::assemble(&body, &root).ok().unwrap();
    assert!(text.contains("Describe these frames"));
    assert_eq!(images.len(), 1);
    assert_eq!(std::fs::read(&images[0]).unwrap(), b"\x89PNG\r\n\x1a\n");
    let bad = serde_json::json!({"messages":[{"content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,!!!!"}}]}]});
    assert!(codex::assemble(&bad, &root).is_err());
}
