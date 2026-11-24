use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
pub struct ResultData {
    pub value: Value,
    pub exit: Option<i32>,
    pub honoured: bool,
    pub hash: Option<String>,
}
pub struct Failure {
    pub status: u16,
    pub exit: Option<i32>,
}
fn drain<R: Read + Send + 'static>(mut r: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut out = Vec::new();
        let mut b = [0; 8192];
        loop {
            match r.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let take = n.min((1024 * 1024usize).saturating_sub(out.len()));
                    out.extend_from_slice(&b[..take]);
                }
            }
        }
        out
    })
}
pub fn disconnected(client: &TcpStream) -> bool {
    let mut b = [0; 1];
    matches!(client.peek(&mut b), Ok(0))
}
/// What one codex child left behind: its JSONL event stream, its stderr and the outcome.
pub struct Ran {
    pub events: Vec<u8>,
    pub stderr: Vec<u8>,
    pub result: Result<i32, Failure>,
    pub reason: &'static str,
}
pub fn run(mut cmd: Command, input: Vec<u8>, client: &TcpStream, timeout: u64) -> Ran {
    let fail = |status, exit, reason| Ran {
        events: Vec::new(),
        stderr: Vec::new(),
        result: Err(Failure { status, exit }),
        reason,
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return fail(502, None, "spawn failed"),
    };
    let pg = child.id() as i32;
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let mut stdin = child.stdin.take().unwrap();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let start = Instant::now();
    let mut failure = None;
    let exit = loop {
        if disconnected(client) || start.elapsed() > Duration::from_millis(timeout) {
            failure = Some(if disconnected(client) { 502 } else { 504 });
            break None;
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => {
                failure = Some(502);
                break None;
            }
        }
        thread::sleep(Duration::from_millis(10));
    };
    // Always terminate descendants too, even if their leader exited successfully.
    unsafe {
        kill(-pg, 15);
    }
    thread::sleep(Duration::from_millis(50));
    unsafe {
        kill(-pg, 9);
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut status = exit;
    while status.is_none() && Instant::now() < deadline {
        status = child.try_wait().ok().flatten();
        thread::sleep(Duration::from_millis(5));
    }
    if status.is_none() {
        return fail(502, None, "child did not exit after kill");
    }
    // Group KILL closes inherited pipes; don't allow drain joins to hang the handler.
    let deadline = Instant::now() + Duration::from_millis(500);
    while !(stdout.is_finished() && stderr.is_finished() && writer.is_finished())
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(5));
    }
    if !(stdout.is_finished() && stderr.is_finished() && writer.is_finished()) {
        return fail(
            502,
            status.and_then(|s| s.code()),
            "pipes held open after exit",
        );
    }
    let events = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    let _ = writer.join();
    let code = status.and_then(|s| s.code().or_else(|| s.signal().map(|n| -n)));
    let (result, reason) = match failure {
        Some(504) => (
            Err(Failure {
                status: 504,
                exit: code,
            }),
            "timeout",
        ),
        Some(s) => (
            Err(Failure {
                status: s,
                exit: code,
            }),
            "client disconnected or wait failed",
        ),
        None if code != Some(0) => (
            Err(Failure {
                status: 502,
                exit: code,
            }),
            "nonzero exit",
        ),
        None => (Ok(0), ""),
    };
    Ran {
        events,
        stderr: err,
        result,
        reason,
    }
}
/// Replaces secrets with `***`: values of secret-named environment variables, bearer tokens,
/// well-known key prefixes and JSON token fields.
pub fn mask(b: &[u8]) -> Vec<u8> {
    let mut s = String::from_utf8_lossy(b).into_owned();
    for (k, v) in std::env::vars() {
        let k = k.to_ascii_uppercase();
        if v.len() >= 8
            && ["KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"]
                .iter()
                .any(|n| k.contains(n))
        {
            s = s.replace(&v, "***");
        }
    }
    let tok = |c: char| c.is_ascii_alphanumeric() || "._-~+/=".contains(c);
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some((i, c)) = rest.char_indices().find(|&(_, c)| tok(c)) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let n = rest.find(|c| !tok(c)).unwrap_or(rest.len());
        let word = &rest[..n];
        let _ = c;
        let lead = out.to_ascii_lowercase();
        let field = [
            "bearer ",
            "access_token\":\"",
            "refresh_token\":\"",
            "id_token\":\"",
            "api_key\":\"",
            "authorization\":\"",
        ]
        .iter()
        .any(|p| lead.ends_with(p));
        let prefixed = word.len() >= 16
            && ["sk-", "eyJ", "ghp_", "gho_", "github_pat_", "xox", "AIza"]
                .iter()
                .any(|p| word.starts_with(p));
        out.push_str(if field || prefixed { "***" } else { word });
        rest = &rest[n..];
    }
    out.push_str(rest);
    out.into_bytes()
}
/// Keeps a failed request's evidence private to the state folder: `failed/<id>.events.jsonl`,
/// `failed/<id>.stderr.txt` and `failed/<id>.json` (reason, status, exit, output file present).
pub fn keep(
    state: &Path,
    id: &str,
    ran: &Ran,
    status: u16,
    exit: Option<i32>,
    reason: &str,
    output_present: bool,
) {
    let dir = state.join("failed");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    let types: Vec<Value> = ran
        .events
        .split(|b| *b == b'\n')
        .filter_map(|l| serde_json::from_slice::<Value>(l).ok())
        .map(|v| v["type"].clone())
        .collect();
    let summary = json!({"id":id,"status":status,"child_exit":exit,"reason":reason,"output_file_present":output_present,"event_types":types,"events_bytes":ran.events.len(),"stderr_bytes":ran.stderr.len()});
    for (name, bytes) in [
        (format!("{id}.events.jsonl"), mask(&ran.events)),
        (format!("{id}.stderr.txt"), mask(&ran.stderr)),
        (format!("{id}.json"), mask(summary.to_string().as_bytes())),
    ] {
        let path = dir.join(name);
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&path)
        {
            let _ = f.write_all(&bytes);
            let _ = f.sync_all();
        }
    }
}
pub fn binary_hash(bin: &str) -> Option<String> {
    Command::new("/usr/bin/sha256sum")
        .arg(bin)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(str::to_owned))
}
/// Assemble text and explicitly attach every image; malformed parts fail closed.
pub fn assemble(body: &Value, cwd: &Path) -> Result<(String, Vec<std::path::PathBuf>), Failure> {
    let fail = || Failure {
        status: 400,
        exit: None,
    };
    let mut prompt = String::new();
    let mut images = Vec::new();
    for message in body["messages"].as_array().ok_or_else(fail)? {
        prompt.push_str(message["role"].as_str().unwrap_or("user"));
        prompt.push_str(":\n");
        if let Some(text) = message["content"].as_str() {
            prompt.push_str(text);
        } else {
            for part in message["content"].as_array().ok_or_else(fail)? {
                match part["type"].as_str() {
                    Some("text") => {
                        prompt.push_str(part["text"].as_str().ok_or_else(fail)?);
                        prompt.push('\n');
                    }
                    Some("image_url") => {
                        let uri = part["image_url"]["url"].as_str().ok_or_else(fail)?;
                        let (header, encoded) = uri.split_once(',').ok_or_else(fail)?;
                        let suffix = match header {
                            "data:image/png;base64" => "png",
                            "data:image/jpeg;base64" => "jpg",
                            _ => return Err(fail()),
                        };
                        let mut bytes = Vec::new();
                        if encoded.is_empty() || encoded.len() % 4 != 0 {
                            return Err(fail());
                        }
                        for (index, chunk) in
                            encoded.as_bytes().as_chunks::<4>().0.iter().enumerate()
                        {
                            let mut word = 0u32;
                            let padding = chunk.iter().rev().take_while(|&&c| c == b'=').count();
                            if padding > 2 || (padding > 0 && (index + 1) * 4 != encoded.len()) {
                                return Err(fail());
                            }
                            for (i, &c) in chunk.iter().enumerate() {
                                let n = match c {
                                    b'A'..=b'Z' => c - b'A',
                                    b'a'..=b'z' => c - b'a' + 26,
                                    b'0'..=b'9' => c - b'0' + 52,
                                    b'+' => 62,
                                    b'/' => 63,
                                    b'=' if i >= 4 - padding => 0,
                                    _ => return Err(fail()),
                                };
                                word = (word << 6) | u32::from(n);
                            }
                            if (padding == 1 && word & 0xff != 0)
                                || (padding == 2 && word & 0xffff != 0)
                            {
                                return Err(fail());
                            }
                            bytes.extend_from_slice(&word.to_be_bytes()[1..4 - padding]);
                        }
                        if !(bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                            || bytes.starts_with(&[0xff, 0xd8, 0xff]))
                        {
                            return Err(fail());
                        }
                        let path = cwd.join(format!("frame-{}.{}", images.len(), suffix));
                        let mut file = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .mode(0o600)
                            .open(&path)
                            .map_err(|_| fail())?;
                        file.write_all(&bytes).map_err(|_| fail())?;
                        images.push(path);
                    }
                    _ => return Err(fail()),
                }
            }
        }
        prompt.push_str("\n\n");
    }
    Ok((prompt, images))
}
pub fn invoke(
    r: &Value,
    body: &Value,
    state: &Path,
    id: &str,
    client: &TcpStream,
    timeout: u64,
) -> Result<ResultData, Failure> {
    // Codex runs in its private cwd, so `-o` must not be relative to the relay's cwd (T404's 502s, child_exit 0).
    let state = &std::path::absolute(state).map_err(|_| Failure {
        status: 502,
        exit: None,
    })?;
    let cwd = state.join("cwd").join(id);
    fs::create_dir_all(&cwd).map_err(|_| Failure {
        status: 502,
        exit: None,
    })?;
    fs::set_permissions(&cwd, fs::Permissions::from_mode(0o700)).map_err(|_| Failure {
        status: 502,
        exit: None,
    })?;
    let output = state.join(format!("{id}.last"));
    let bin = r["bin"].as_str().unwrap();
    let hash = binary_hash(bin);
    let mut cmd = Command::new(bin);
    cmd.current_dir(&cwd)
        .args([
            "exec",
            "--ephemeral",
            "--ignore-user-config",
            "--skip-git-repo-check",
            "-s",
            "read-only",
            "-m",
            r["model"].as_str().unwrap(),
            "-c",
            &format!(
                "model_reasoning_effort=\"{}\"",
                r["effort"].as_str().unwrap()
            ),
            "--json",
            "-o",
        ])
        .arg(&output)
        .arg("-");
    let (prompt, images) = assemble(body, &cwd)?;
    for image in &images {
        cmd.arg("--image").arg(image);
    }
    let ran = run(cmd, prompt.into_bytes(), client, timeout);
    let lost = |status, exit, reason: &str| {
        keep(state, id, &ran, status, exit, reason, output.exists());
        Failure { status, exit }
    };
    let exit = match &ran.result {
        Ok(e) => *e,
        Err(f) => return Err(lost(f.status, f.exit, ran.reason)),
    };
    let mut model = Value::Null;
    let mut usage = json!({});
    let mut finish = "stop";
    for v in ran
        .events
        .split(|b| *b == b'\n')
        .filter_map(|l| serde_json::from_slice::<Value>(l).ok())
    {
        if let Some(m) = v["model"].as_str() {
            model = json!(m)
        }
        if v["type"] == "turn.failed" || v["type"] == "error" {
            return Err(lost(502, Some(exit), "turn.failed or error event"));
        }
        if v["type"] == "turn.completed" {
            usage = v["usage"].clone();
        }
        if v["finish_reason"] == "length" {
            finish = "length"
        }
    }
    let content =
        fs::read_to_string(&output).map_err(|_| lost(502, Some(exit), "no output file"))?;
    usage["attached_images"] = json!(images.len());
    let receipt =
        json!({"id":id,"attached_images":images.len(),"image_files":images,"child_exit":exit});
    fs::write(
        state.join(format!("{id}.images-receipt.json")),
        receipt.to_string(),
    )
    .map_err(|_| lost(502, Some(exit), "image receipt could not be written"))?;
    Ok(ResultData {
        value: json!({"choices":[{"message":{"content":content},"finish_reason":finish}],"model":model,"usage":usage}),
        exit: Some(exit),
        honoured: false,
        hash,
    })
}
