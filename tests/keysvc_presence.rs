//! QROC against the key service, on fixtures only: route availability (`key_missing`) and request
//! admission (`build_openrouter_request`) follow the service's presence answer once the layer's
//! switch file exists, and never fall back to a readable or poisoned legacy key file. Synthetic keys
//! and a temporary service store; the service binary is named by `STRATUM_KEYSVC_BIN`.
use qroc_standalone::routes::{
    Kind, LAYER_CHAT, OPENROUTER_ENDPOINT, Route, build_openrouter_request, key_missing,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use stratum_api::keysvc;

const KEY: &str = "OPENROUTER_API_KEY";
const LEGACY: &str = "OPENROUTER_API_KEY=sk-fixture-legacy-SENTINEL\n";

struct Service(Option<Child>);
impl Service {
    fn start(store: &Path, socket: &Path) -> Self {
        let bin =
            std::env::var("STRATUM_KEYSVC_BIN").expect("STRATUM_KEYSVC_BIN names the service");
        let child = Command::new(bin)
            .arg("serve")
            .arg(store)
            .arg(socket)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !socket.exists() {
            assert!(Instant::now() < until, "service failed to start");
            thread::sleep(Duration::from_millis(10));
        }
        Self(Some(child))
    }
    fn stop(mut self) {
        let mut child = self.0.take().unwrap();
        let _ = child.kill();
        let out = child.wait_with_output().unwrap();
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn private_dir(p: &Path) {
    fs::create_dir(p).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
}
fn legacy(config: &Path) {
    let path = config.join("keys.env");
    fs::write(&path, LEGACY).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn route() -> Route {
    Route {
        name: "openrouter".into(),
        kind: Kind::Openrouter,
        endpoint: OPENROUTER_ENDPOINT.into(),
        model: "openai/gpt-oss-120b".into(),
        key_ref: Some(KEY.into()),
    }
}
/// (route available, request admitted), and the admitted request never carries a key.
fn state(config: &Path) -> (bool, bool) {
    let c = config.to_string_lossy().into_owned();
    let env = |k: &str| (k == "STRATUM_API_CONFIG").then(|| c.clone());
    let available = !key_missing(&route(), &env);
    let request =
        build_openrouter_request(&route(), &json!([{"role":"user","content":"hi"}]), 16, &env);
    match &request {
        // Admitted: sent to the shared layer, which adds the key itself; nothing shown holds one.
        Ok(r) => {
            assert_eq!(r.url, LAYER_CHAT);
            assert!(!r.masked().to_string().contains("SENTINEL"));
        }
        Err(e) => assert!(!e.to_string().contains("SENTINEL")),
    }
    (available, request.is_ok())
}

#[test]
#[ignore = "needs the key service binary in STRATUM_KEYSVC_BIN; run by the key service install check"]
fn route_and_admission_follow_the_key_service_and_never_fall_back() {
    // Scratch under TMPDIR, kept afterwards (nothing is deleted).
    let dir = std::env::temp_dir().join(format!("qroc-keysvc-{}", std::process::id()));
    private_dir(&dir);
    let (store, run, config) = (dir.join("store"), dir.join("run"), dir.join("config"));
    private_dir(&store);
    private_dir(&run);
    private_dir(&config);
    let socket = run.join("s");

    // Before the switch: the old readable key file answers, as today.
    legacy(&config);
    assert_eq!(state(&config), (true, true), "legacy before the switch");

    fs::write(
        config.join("keysvc.json"),
        json!({"socket": socket}).to_string(),
    )
    .unwrap();
    let service = Service::start(&store, &socket);
    // Switched, service key absent, readable legacy file present: not available, refused.
    assert_eq!(
        state(&config),
        (false, false),
        "removed/absent, old readable legacy"
    );

    keysvc::manage(&socket, "set", KEY, Some("sk-fixture-service-SENTINEL")).unwrap();
    assert_eq!(state(&config), (true, true), "service key present");

    // Poisoned legacy paths do not matter while the service answers.
    fs::rename(config.join("keys.env"), config.join("keys.env.kept")).unwrap();
    symlink("/nonexistent/keys.env", config.join("keys.env")).unwrap();
    assert_eq!(
        state(&config),
        (true, true),
        "present, legacy is a dangling link"
    );
    fs::rename(config.join("keys.env"), config.join("keys.env.link")).unwrap();
    fs::write(config.join("keys.env"), "garbage\0\n").unwrap();
    fs::set_permissions(config.join("keys.env"), fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(state(&config), (true, true), "present, legacy unreadable");

    keysvc::manage(&socket, "remove", KEY, None).unwrap();
    assert_eq!(
        state(&config),
        (false, false),
        "service key removed, poisoned legacy"
    );

    keysvc::manage(&socket, "set", KEY, Some("sk-fixture-service-SENTINEL")).unwrap();
    service.stop();
    // Service unavailable with an old readable legacy file: no fallback.
    fs::set_permissions(config.join("keys.env"), fs::Permissions::from_mode(0o600)).unwrap();
    legacy(&config);
    assert_eq!(
        state(&config),
        (false, false),
        "service unavailable, readable legacy"
    );
    // A broken switch file is unavailable too, never the legacy file.
    fs::write(config.join("keysvc.json"), "{not json").unwrap();
    assert_eq!(
        state(&config),
        (false, false),
        "malformed switch, readable legacy"
    );
}
