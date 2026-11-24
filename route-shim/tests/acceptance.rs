#[test]
fn fake_codex_budgets_disconnect_timeout_key_redaction_plan_and_status() {
    let output = std::process::Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/acceptance.py"))
        .arg(env!("CARGO_BIN_EXE_route-shim"))
        .output()
        .unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
