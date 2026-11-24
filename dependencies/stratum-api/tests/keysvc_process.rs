//! The real service binary, under the unprivileged test user's identity in a temporary store.
//! No real account, key, installed setting or provider is touched.
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use stratum_api::keysvc;
struct Process(Option<Child>);
impl Process {
    fn stop(mut self) -> std::process::Output {
        let mut child = self.0.take().unwrap();
        let _ = child.kill();
        child.wait_with_output().unwrap()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[test]
fn real_binary_management_restart_and_no_secret_output() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let store = dir.path().join("store");
    fs::create_dir(&store).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    // Separate runtime names permit restarting without deleting the preserved first socket.
    let start = |n: &str| {
        let runtime = dir.path().join(n);
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = runtime.join("service.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_stratum-keysvc"))
            .arg("serve")
            .arg(&store)
            .arg(&socket)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let process = Process(Some(child));
        let until = Instant::now() + Duration::from_secs(3);
        while !socket.exists() {
            assert!(Instant::now() < until, "service failed to start");
            thread::sleep(Duration::from_millis(10));
        }
        (process, socket)
    };
    let (process, socket) = start("run1");
    assert_eq!(
        fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o660
    );
    let result = keysvc::manage(
        &socket,
        "set",
        "OPENAI_API_KEY",
        Some("sk-fixture-PREFIX!TAIL-SENTINEL"),
    )
    .unwrap();
    assert!(!result.to_string().contains("SENTINEL"));
    assert_eq!(
        keysvc::manage(&socket, "status", "OPENAI_API_KEY", None).unwrap()["set"],
        true
    );
    assert!(keysvc::manage(&socket, "get", "OPENAI_API_KEY", None).is_err());
    let output = process.stop();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let (process, socket) = start("run2");
    assert_eq!(
        keysvc::manage(&socket, "status", "OPENAI_API_KEY", None).unwrap()["set"],
        true
    );
    assert_eq!(
        keysvc::manage(&socket, "remove", "OPENAI_API_KEY", None).unwrap()["set"],
        false
    );
    let output = process.stop();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

// Compile the exact deployed QROC call site against this candidate's public compatibility API.
mod qroc_presence {
    use std::path::PathBuf;
    include!("fixtures/qroc_key_presence.rs");
    pub fn configured(config: &std::path::Path) -> bool {
        layer_has_key(&|name| match name {
            "STRATUM_API_CONFIG" => Some(config.to_string_lossy().into_owned()),
            _ => None,
        })
    }
}
#[test]
fn deployed_qroc_presence_call_uses_service_and_fails_closed_without_legacy_fallback() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let store = dir.path().join("store");
    fs::create_dir(&store).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.path().join("service.sock");
    let child = Command::new(env!("CARGO_BIN_EXE_stratum-keysvc"))
        .arg("serve")
        .arg(&store)
        .arg(&socket)
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let process = Process(Some(child));
    let until = Instant::now() + Duration::from_secs(3);
    while !socket.exists() {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    let config = dir.path().join("config");
    fs::create_dir(&config).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        config.join("keysvc.json"),
        serde_json::json!({"socket":socket}).to_string(),
    )
    .unwrap();
    // A valid legacy fixture must never substitute for an absent/removed/broken service credential.
    let legacy = config.join("keys.env");
    fs::write(&legacy, "OPENROUTER_API_KEY=fixture-legacy-only\n").unwrap();
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!qroc_presence::configured(&config));
    keysvc::manage(
        &socket,
        "set",
        "OPENROUTER_API_KEY",
        Some("fixture-service-only"),
    )
    .unwrap();
    assert!(qroc_presence::configured(&config));
    keysvc::manage(&socket, "remove", "OPENROUTER_API_KEY", None).unwrap();
    assert!(!qroc_presence::configured(&config));
    keysvc::manage(
        &socket,
        "set",
        "OPENROUTER_API_KEY",
        Some("fixture-service-only"),
    )
    .unwrap();
    // Poison the legacy path; both public presence APIs must still succeed using the service.
    fs::rename(&legacy, config.join("legacy-fixture.kept")).unwrap();
    fs::create_dir(&legacy).unwrap();
    assert!(qroc_presence::configured(&config));
    assert!(
        stratum_api::Layer::at(dir.path().join("state"), &config).has_key("OPENROUTER_API_KEY")
    );
    fs::rename(
        config.join("legacy-fixture.kept"),
        config.join("readable-fixture.env"),
    )
    .unwrap();
    // A custom legacy pathname must obey the same service switch.
    let readable = config.join("readable-fixture.env");
    assert!(stratum_api::keys::present(&readable, "OPENROUTER_API_KEY"));
    fs::write(config.join("keysvc.json"), "malformed").unwrap();
    assert!(!stratum_api::keys::present(&readable, "OPENROUTER_API_KEY"));
    fs::write(
        config.join("keysvc.json"),
        serde_json::json!({"socket":socket}).to_string(),
    )
    .unwrap();
    let output = process.stop();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(!stratum_api::keys::present(&readable, "OPENROUTER_API_KEY"));
    // Legacy callers without a switch retain their exact file-path behavior.
    fs::rename(
        config.join("keysvc.json"),
        config.join("keysvc-switch.kept"),
    )
    .unwrap();
    assert!(stratum_api::keys::present(&readable, "OPENROUTER_API_KEY"));
}

#[test]
fn service_command_error_is_fixed_and_does_not_echo_rejected_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_stratum-keysvc"))
        .arg("sk-fixture-PREFIX!TAIL-SENTINEL")
        .env_clear()
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"{\"error\":\"key service failed\"}\n");
}

#[test]
fn real_binary_imports_legacy_text_from_standard_input_without_output_or_a_file() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let store = dir.path().join("store");
    fs::create_dir(&store).unwrap();
    fs::set_permissions(&store, fs::Permissions::from_mode(0o700)).unwrap();
    let import = |text: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stratum-keysvc"))
            .args(["import-legacy", store.to_str().unwrap(), "-"])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(text).unwrap();
        child.wait_with_output().unwrap()
    };
    let output = import(b"OPENROUTER_API_KEY=sk-fixture-PREFIX!TAIL-SENTINEL\n");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(!store.join("legacy.env").exists());
    let stored = fs::read(store.join("store.aead")).unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains("SENTINEL"));
    // A second import of the same name is refused with the fixed reply and no echo.
    let output = import(b"OPENROUTER_API_KEY=sk-fixture-other-SENTINEL\n");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"{\"error\":\"key service failed\"}\n");
    let runtime = dir.path().join("run");
    fs::create_dir(&runtime).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = runtime.join("service.sock");
    let process = Process(Some(
        Command::new(env!("CARGO_BIN_EXE_stratum-keysvc"))
            .arg("serve")
            .arg(&store)
            .arg(&socket)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let until = Instant::now() + Duration::from_secs(3);
    while !socket.exists() {
        assert!(Instant::now() < until, "service failed to start");
        thread::sleep(Duration::from_millis(10));
    }
    let status = keysvc::manage(&socket, "status", "OPENROUTER_API_KEY", None).unwrap();
    assert_eq!(status["set"], true);
    assert!(!status.to_string().contains("SENTINEL"));
    let output = process.stop();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn layer_status_shows_each_provider_present_with_last_use_and_no_value() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let (store, runtime, config, home) = (
        dir.path().join("store"),
        dir.path().join("run"),
        dir.path().join("config"),
        dir.path().join("home"),
    );
    for d in [&store, &runtime, &config, &home] {
        fs::create_dir(d).unwrap();
        fs::set_permissions(d, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let socket = runtime.join("service.sock");
    let process = Process(Some(
        Command::new(env!("CARGO_BIN_EXE_stratum-keysvc"))
            .arg("serve")
            .arg(&store)
            .arg(&socket)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let until = Instant::now() + Duration::from_secs(3);
    while !socket.exists() {
        assert!(Instant::now() < until, "service failed to start");
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(
        config.join("keysvc.json"),
        serde_json::json!({"socket": socket}).to_string(),
    )
    .unwrap();
    keysvc::manage(
        &socket,
        "set",
        "OPENAI_API_KEY",
        Some("sk-fixture-status-SENTINEL"),
    )
    .unwrap();
    let status = |json: bool| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_stratum-api"));
        c.arg("status")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("STRATUM_API_CONFIG", &config)
            .env("STRATUM_API_HOME", &home);
        if json {
            c.arg("--json");
        }
        let out = c.output().unwrap();
        assert!(!String::from_utf8_lossy(&out.stdout).contains("SENTINEL"));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("SENTINEL"));
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };
    let text = status(false);
    assert!(text.contains("Key service: available"), "{text}");
    assert!(
        text.contains("  OPENAI_API_KEY: present, not used yet"),
        "{text}"
    );
    assert!(text.contains("  ANTHROPIC_API_KEY: not set"), "{text}");
    assert!(text.contains("  OPENROUTER_API_KEY: not set"), "{text}");
    let v: serde_json::Value = serde_json::from_str(&status(true)).unwrap();
    assert_eq!(v["settings"]["keys"]["OPENAI_API_KEY"]["set"], true);
    assert_eq!(v["settings"]["keys"]["ANTHROPIC_API_KEY"]["set"], false);
    let output = process.stop();
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    // With the service gone the status says so and still shows no value.
    assert!(status(false).contains("Key service: unavailable"));
}
