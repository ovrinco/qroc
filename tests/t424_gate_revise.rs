//! Revise from gate (T424): a failed gate's joined documents are regenerated with the gate's
//! criterion feedback, then the gate is regenerated and evaluated, at most two rounds per call.
//! Every round is the operator's own press (design record entry 42); questions only he can
//! answer are kept and hold their documents. The gateway is a loopback stand-in on port 0;
//! nothing reaches a provider.
#[path = "common/hardening.rs"]
mod hardening;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const FAIL_81: &str = "[SCORE]: 81\n- Epic Coverage: 24/30 - epics miss the export and undo flows\n- Slice Independence: 20/25 - slices 3 and 4 share mutable state\n- Slice Acceptance: 20/25 - acceptance lacks measurable limits\n- Traceability: 17/20 - two slices lack PRD links";
const FAIL_83: &str = "[SCORE]: 83\n- Epic Coverage: 25/30 - export flow still missing\n- Slice Independence: 21/25 - slice 4 still shares state\n- Slice Acceptance: 20/25 - limits still unmeasured\n- Traceability: 17/20 - links still missing";
const PASS_92: &str = "[SCORE]: 92\n- Epic Coverage: 28/30 - every flow has an epic\n- Slice Independence: 23/25 - slices stand alone\n- Slice Acceptance: 23/25 - acceptance is measurable\n- Traceability: 18/20 - each slice cites the PRD";

fn root(name: &str) -> PathBuf {
    let base = std::env::var_os("QROC_TEST_ROOT").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("test-roots")
        },
        PathBuf::from,
    );
    base.join(format!(
        "t424-{}-{name}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// Answers `count` requests: a gate prompt takes the next gate text, any other a plain draft.
fn gateway(count: usize, gates: Vec<&'static str>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let mut gates = gates.into_iter();
        let mut prompts = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.strip_prefix("Content-Length: ") {
                    length = n.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let prompt = request["messages"][1]["content"]
                .as_str()
                .unwrap()
                .to_owned();
            let content = if prompt.contains("\nDocument: GATE_3\n")
                || prompt.contains("\nDocument: GATE_2\n")
                || prompt.contains("\nDocument: GATE_1\n")
            {
                gates.next().expect("a gate text for every gate request")
            } else {
                "A revised stand-in draft."
            };
            let body = json!({"model":"test","choices":[{"message":{"content":content},"finish_reason":"stop"}]}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            prompts.push(prompt);
        }
        prompts
    });
    (endpoint, server)
}

struct Engine {
    child: Child,
    out: BufReader<ChildStdout>,
    sequence: u64,
    root: PathBuf,
}

impl Engine {
    fn start(name: &str, endpoint: &str) -> Self {
        Self::open_at(root(name), endpoint, "1024")
    }
    /// The engine on an existing root with a declared output limit.
    fn open_at(root: PathBuf, endpoint: &str, max_tokens: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qroc-bridge"))
            .args(["--root", root.to_str().unwrap()])
            .env("QROC_GATEWAY", endpoint)
            .env("QROC_MODEL", "test")
            .env("QROC_PROVIDER", "local-gateway")
            .env("QROC_MAX_TOKENS", max_tokens)
            .env("QROC_TIMEOUT_SECONDS", "45")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        Self {
            child,
            out,
            sequence: 0,
            root,
        }
    }
    fn reply(&mut self, command: &str, args: Value) -> Value {
        self.sequence += 1;
        writeln!(
            self.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"version":1,"id":self.sequence.to_string(),"command":command,"args":args})
        )
        .unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn ask(&mut self, command: &str, args: Value) -> Value {
        let reply = self.reply(command, args);
        assert_eq!(reply["ok"], true, "{reply}");
        reply["result"].clone()
    }
    /// A project and branch with Epics, Slice index and Gate 3 at revision 1.
    fn fixture(&mut self, gate: &str) -> Value {
        let project = self.ask(
            "qroc-project-create",
            json!({"title":"Fixture","brain_dump":"Private stand-in idea"}),
        );
        hardening::pass_hardening(&self.root.join("qroc"));
        let branch = self.ask(
            "qroc-branch-create",
            json!({"project_id":project["id"],"name":"Fixture","provider":"local-gateway","model":"test"}),
        );
        let owner = json!({"project_id":project["id"],"branch_id":branch["id"]});
        for kind in [
            "STRATEGY_BRIEF",
            "PRE_MORTEM",
            "PRD",
            "VALIDATION_PLAN",
            "TECHNICAL_ARCHITECTURE",
            "DESIGN_ALTERNATIVES",
            "UI_UX_SPEC",
            "ROADMAP",
            "IMPLEMENTATION_PLAN",
        ] {
            let mut args = owner.clone();
            args["document_type"] = json!(kind);
            args["content"] = json!(format!("Fixture {kind} r1."));
            self.ask("qroc-document-save", args);
        }
        for (kind, content) in [
            ("EPICS", "Stand-in epics r1."),
            ("SLICE_INDEX", "Stand-in slices r1."),
            ("GATE_3", gate),
        ] {
            let mut args = owner.clone();
            args["document_type"] = json!(kind);
            args["content"] = json!(content);
            self.ask("qroc-document-save", args);
        }
        owner
    }
    fn evaluate(&mut self, owner: &Value, revision: u64) -> Value {
        let mut args = owner.clone();
        args["document_type"] = json!("GATE_3");
        args["revision"] = json!(revision);
        self.ask("qroc-gate-evaluate", args)
    }
    fn revise_until_done(&mut self, owner: &Value) -> Value {
        let mut args = owner.clone();
        args["gate"] = json!("GATE_3");
        for _ in 0..1000 {
            let state = self.ask("qroc-gate-revise-get", args.clone());
            if state["running"] == false {
                return state;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("revise did not finish");
    }
    fn stop(mut self) {
        self.child.stdin.take();
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn failed_gate_passes_after_one_round_with_two_regenerations() {
    let (endpoint, server) = gateway(3, vec![PASS_92]);
    let mut engine = Engine::start("pass", &endpoint);
    let owner = engine.fixture(FAIL_81);
    let before = engine.evaluate(&owner, 1);
    assert_eq!(
        (&before["status"], &before["score"], &before["threshold"]),
        (&json!("failed"), &json!(81), &json!(90))
    );
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    let started = engine.ask("qroc-gate-revise-start", args.clone());
    assert_eq!(started["running"], true);
    let done = engine.revise_until_done(&owner);
    let call = &done["latest"];
    println!(
        "T424 recorded call: {}",
        serde_json::to_string_pretty(call).unwrap()
    );
    assert_eq!(call["state"], "passed");
    assert_eq!(call["threshold"], 90);
    assert_eq!(call["rounds"].as_array().unwrap().len(), 1);
    let round = &call["rounds"][0];
    assert_eq!(round["round"], 1);
    assert_eq!(round["answers_gate_revision"], 1);
    assert_eq!(round["gate_revision"], 2);
    assert_eq!(round["score"], 92);
    assert_eq!(round["status"], "passed");
    let documents = round["documents"].as_array().unwrap();
    assert_eq!(documents.len(), 2);
    for doc in documents {
        assert_eq!(doc["state"], "succeeded");
        assert_eq!(doc["revision"], 2);
        let mut detail = owner.clone();
        detail["document_type"] = doc["document_type"].clone();
        detail["revision"] = json!(2);
        let saved = engine.ask("qroc-document-detail", detail);
        let tag = &saved["generation_inputs"]["gate_revise"];
        assert_eq!(
            (
                &tag["gate"],
                &tag["round"],
                &tag["of"],
                &tag["answers_gate_revision"]
            ),
            (&json!("GATE_3"), &json!(1), &json!(2), &json!(1))
        );
        assert!(
            tag["feedback"]
                .as_str()
                .unwrap()
                .contains("export and undo flows")
        );
    }
    let mut gate_detail = owner.clone();
    gate_detail["document_type"] = json!("GATE_3");
    gate_detail["revision"] = json!(2);
    let gate_saved = engine.ask("qroc-document-detail", gate_detail);
    let inputs = &gate_saved["generation_inputs"];
    for kind in ["EPICS", "SLICE_INDEX"] {
        assert_eq!(inputs["consumed_documents"][kind]["revision"], 2);
        assert_eq!(
            inputs["gate_revise"]["consumed_baseline"][kind],
            Value::Null
        );
        assert!(
            !inputs["context"]
                .as_str()
                .unwrap()
                .contains(&format!("Revision baseline {kind} revision 1"))
        );
    }
    let after = engine.evaluate(&owner, 2);
    assert_eq!(
        (&after["status"], &after["score"], &after["threshold"]),
        (&json!("passed"), &json!(92), &json!(90))
    );
    let graph = engine.ask("qroc-graph-export", owner.clone());
    let node = |id: &str| {
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()
            .clone()
    };
    for id in ["GATE_3", "EPICS", "SLICE_INDEX"] {
        let revise = &node(id)["revise"];
        assert_eq!(revise["label"], "round 1 of 2", "{id}");
        assert_eq!(revise["state"], "passed", "{id}");
    }
    assert_eq!(node("PRD")["revise"], Value::Null);
    let again = engine.reply("qroc-gate-revise-start", args);
    assert!(again["error"].as_str().unwrap().contains("has passed"));
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 3);
    for prompt in &prompts[..2] {
        assert!(prompt.contains("Gate feedback to answer"));
        assert!(prompt.contains("revised one after another from one shared, current baseline"));
        assert!(!prompt.contains("historical drafts"));
        assert!(prompt.contains("slices 3 and 4 share mutable state"));
    }
    assert!(prompts[2].contains("\nDocument: GATE_3\n"));
    assert!(!prompts[2].contains("Gate feedback to answer"));
}

#[test]
fn second_round_waits_for_a_press_and_an_exhausted_call_stays_failed() {
    let (endpoint, server) = gateway(6, vec![FAIL_83, FAIL_83]);
    let mut engine = Engine::start("fail", &endpoint);
    let owner = engine.fixture(FAIL_81);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    engine.ask("qroc-gate-revise-start", args.clone());
    // Round 1 fails; the engine starts nothing more until his press.
    let first = engine.revise_until_done(&owner);
    let call = &first["latest"];
    assert_eq!(call["state"], "failed");
    assert_eq!(call["awaiting_press"], true);
    assert_eq!(first["awaiting_press"], true);
    assert_eq!(call["rounds"].as_array().unwrap().len(), 1);
    assert_eq!(call["last"]["score"], 83);
    for _ in 0..20 {
        engine.ask("qroc-job-list", owner.clone());
        thread::sleep(Duration::from_millis(10));
    }
    let jobs = engine.ask("qroc-job-list", owner.clone());
    assert_eq!(
        jobs["jobs"].as_array().unwrap().len(),
        3,
        "no round without a press"
    );
    let graph = engine.ask("qroc-graph-export", owner.clone());
    let gate = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "GATE_3")
        .unwrap();
    assert_eq!(gate["revise"]["label"], "round 1 of 2");
    assert_eq!(gate["revise"]["state"], "failed");
    // His second press continues the same call with round 2.
    let pressed = engine.ask("qroc-gate-revise-start", args.clone());
    assert_eq!(pressed["call"]["call"], 1);
    let done = engine.revise_until_done(&owner);
    assert_eq!(done["record"]["calls"].as_array().unwrap().len(), 1);
    let call = &done["latest"];
    assert_eq!(call["state"], "failed");
    assert_eq!(call["awaiting_press"], false);
    let rounds = call["rounds"].as_array().unwrap();
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0]["answers_gate_revision"], 1);
    assert_eq!(rounds[1]["answers_gate_revision"], 2);
    assert_eq!(rounds[1]["gate_revision"], 3);
    assert_eq!(call["last"]["score"], 83);
    assert_eq!(call["last"]["status"], "failed");
    assert_eq!(call["last"]["threshold"], 90);
    assert!(
        call["last"]["criteria"]["epic coverage"]["feedback"]
            .as_str()
            .unwrap()
            .contains("export flow")
    );
    let graph = engine.ask("qroc-graph-export", owner.clone());
    let gate = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "GATE_3")
        .unwrap();
    assert_eq!(gate["revise"]["label"], "round 2 of 2");
    assert_eq!(gate["revise"]["state"], "failed");
    assert_eq!(gate["gate"]["threshold"], 90);
    let exhausted = engine.evaluate(&owner, 3);
    assert_eq!(
        (&exhausted["status"], &exhausted["threshold"]),
        (&json!("failed"), &json!(90))
    );
    // Polling never softens the exhausted call.
    for _ in 0..5 {
        assert_eq!(
            engine.revise_until_done(&owner)["latest"]["state"],
            "failed"
        );
    }
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 6);
    // Round 2 answers the regenerated gate's feedback, not the first one.
    assert!(prompts[3].contains("export flow still missing"));
}

const HELD_88: &str = "[SCORE]: 88\n- Epic Coverage: 30/30 - every flow has an epic\n- Slice Independence: 25/25 - slices stand alone\n- Slice Acceptance: 25/25 - acceptance is measurable\n- Traceability: 8/20 - slices cannot name the phone they serve\n\n## Blocking Gaps\n1. [QUESTION] Traceability | SLICE_INDEX | Which phone model and system version will the first version use?\n\n## Required Remediation\n1. None a writer can make without that answer.";
const MIXED_84: &str = "[SCORE]: 84\n- Epic Coverage: 24/30 - epics miss the export flow\n- Slice Independence: 25/25 - slices stand alone\n- Slice Acceptance: 25/25 - acceptance is measurable\n- Traceability: 10/20 - slices cannot name the phone they serve\n\n## Blocking Gaps\n- [QUESTION] Traceability | SLICE_INDEX | Which phone model and system version will the first version use?\n- [QUESTION] Made Up Criterion | SLICE_INDEX | Not a criterion of this gate\n\n## Required Remediation\n- [REPAIR] EPICS | Epic Coverage | add the export flow epic";

#[test]
fn operator_question_is_kept_and_no_round_is_spent_on_its_document() {
    let (endpoint, server) = gateway(2, vec![FAIL_83]);
    let mut engine = Engine::start("question-held", &endpoint);
    let owner = engine.fixture(MIXED_84);
    let verdict = engine.evaluate(&owner, 1);
    assert_eq!(verdict["status"], "failed");
    assert_eq!(verdict["questions"].as_array().unwrap().len(), 1);
    assert_eq!(verdict["questions"][0]["criterion"], "traceability");
    assert_eq!(verdict["questions"][0]["document"], "SLICE_INDEX");
    assert_eq!(verdict["repairs"][0]["document"], "EPICS");
    assert_eq!(
        verdict["questions_note"],
        "1 question only the person can answer holds back traceability; no answer was invented. 1 question or repair line could not be read and was not kept."
    );
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    engine.ask("qroc-gate-revise-start", args.clone());
    let done = engine.revise_until_done(&owner);
    assert_eq!(done["open_questions"], 1);
    assert_eq!(done["questions"][0]["standing"], "open");
    assert_eq!(done["questions"][0]["answer"], Value::Null);
    let round = &done["latest"]["rounds"][0];
    let revised = round["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["document_type"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(revised, ["EPICS"]);
    assert_eq!(round["held_documents"][0]["document_type"], "SLICE_INDEX");
    assert_eq!(round["held_documents"][0]["questions"][0], "question-1");
    let mut slices = owner.clone();
    slices["document_type"] = json!("SLICE_INDEX");
    slices["revision"] = json!(2);
    assert_eq!(engine.reply("qroc-document-detail", slices)["ok"], false);
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 2, "EPICS and the gate only");
    assert!(prompts[0].contains("Questions only the project's owner can answer"));
    assert!(prompts[0].contains("Which phone model and system version"));
    assert!(prompts[1].contains("[QUESTION] <criterion name>"));
}

#[test]
fn press_is_refused_when_every_shortfall_is_his_question_until_he_answers() {
    let (endpoint, server) = gateway(3, vec![PASS_92]);
    let mut engine = Engine::start("question-refused", &endpoint);
    let owner = engine.fixture(HELD_88);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    let refused = engine.reply("qroc-gate-revise-start", args.clone());
    assert_eq!(refused["ok"], false);
    assert_eq!(
        refused["error"],
        "No round was started: every remaining shortfall waits on a question only the person can answer: Which phone model and system version will the first version use? (blocks traceability)"
    );
    let state = engine.ask("qroc-gate-revise-get", args.clone());
    assert_eq!(state["open_questions"], 1);
    assert_eq!(state["record"]["calls"].as_array().unwrap().len(), 0);
    assert_eq!(
        engine.ask("qroc-job-list", owner.clone())["jobs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let mut answer = args.clone();
    answer["question_id"] = json!("question-1");
    let blank = engine.reply("qroc-gate-question-answer", {
        let mut a = answer.clone();
        a["answer"] = json!("  ");
        a
    });
    assert_eq!(blank["ok"], false);
    answer["answer"] = json!("A Pixel 9 on Android 16.");
    let settled = engine.ask("qroc-gate-question-answer", answer.clone());
    assert_eq!(settled["question"]["standing"], "answered");
    assert_eq!(settled["open_questions"], 0);
    assert_eq!(
        engine.reply("qroc-gate-question-answer", answer)["ok"],
        false
    );
    // With his answer the press runs, and the answer reaches the drafts as his.
    engine.ask("qroc-gate-revise-start", args);
    let done = engine.revise_until_done(&owner);
    assert_eq!(done["latest"]["state"], "passed");
    assert_eq!(done["questions"][0]["standing"], "answered");
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].contains("A Pixel 9 on Android 16."));
    assert!(!prompts[0].contains("Questions only the project's owner can answer"));
}

#[test]
fn passed_or_never_evaluated_gate_is_refused_without_a_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let mut engine = Engine::start("refuse", &endpoint);
    let owner = engine.fixture(PASS_92);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    let refused = engine.reply("qroc-gate-revise-start", args.clone());
    assert_eq!(refused["ok"], false);
    assert_eq!(
        refused["error"],
        "This gate has passed; there is nothing to revise"
    );
    args["gate"] = json!("GATE_2");
    let refused = engine.reply("qroc-gate-revise-start", args.clone());
    assert_eq!(
        refused["error"],
        "This gate has not been evaluated yet, so there is no feedback to revise from"
    );
    let mut forged = owner.clone();
    forged["document_type"] = json!("EPICS");
    forged["gate_revise"] = json!({"round":1});
    assert_eq!(engine.reply("qroc-generate-start", forged)["ok"], false);
    assert_eq!(engine.evaluate(&owner, 1)["threshold"], 90);
    assert_eq!(
        std::fs::read_dir(engine.root.join("qroc/jobs"))
            .unwrap()
            .count(),
        0
    );
    engine.stop();
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "no request reached the gateway");
}

#[test]
fn gate_one_rebinds_prd_and_architecture_after_the_round_completes() {
    fn gate_text(score: u32) -> &'static str {
        let rubric = qroc_standalone::gate_rubric::rubric("GATE_1")
            .unwrap()
            .unwrap();
        let mut content = format!("[SCORE]: {score}\n");
        for c in rubric.criteria {
            content.push_str(&format!(
                "- {}: {}/{} - fixture criterion feedback\n",
                c.name,
                c.max * score / 100,
                c.max
            ));
        }
        Box::leak(content.into_boxed_str())
    }
    let (endpoint, server) = gateway(6, vec![gate_text(100)]);
    let mut engine = Engine::start("gate-one-bindings", &endpoint);
    let owner = engine.fixture(FAIL_81);
    let mut args = owner.clone();
    args["document_type"] = json!("GATE_1");
    args["content"] = json!(gate_text(80));
    engine.ask("qroc-document-save", args);
    let mut revise = owner.clone();
    revise["gate"] = json!("GATE_1");
    revise["context"] = json!("Supplementary source answer: start only with explicit intent.");
    engine.ask("qroc-gate-revise-start", revise.clone());
    for _ in 0..1000 {
        if engine.ask("qroc-gate-revise-get", revise.clone())["running"] == false {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let mut args = owner.clone();
    args["document_type"] = json!("GATE_1");
    args["revision"] = json!(2);
    let gate = engine.ask("qroc-document-detail", args);
    let context = gate["generation_inputs"]["context"].as_str().unwrap();
    for kind in ["PRD", "TECHNICAL_ARCHITECTURE"] {
        assert!(
            !context.contains(&format!("Baseline {kind} revision 1")),
            "{context}"
        );
        assert_eq!(
            gate["generation_inputs"]["consumed_documents"][kind]["revision"],
            2
        );
        let mut args = owner.clone();
        args["document_type"] = json!(kind);
        args["revision"] = json!(2);
        let sibling = engine.ask("qroc-document-detail", args);
        assert_eq!(
            gate["generation_inputs"]["consumed_documents"][kind]["sha256"],
            sibling["sha256"]
        );
        assert_eq!(
            gate["generation_inputs"]["consumed_documents"][kind]["id"],
            sibling["id"]
        );
    }
    let mut validation_args = owner.clone();
    validation_args["document_type"] = json!("VALIDATION_PLAN");
    validation_args["revision"] = json!(2);
    let validation = engine.ask("qroc-document-detail", validation_args);
    for kind in ["PRE_MORTEM", "TECHNICAL_ARCHITECTURE"] {
        assert_eq!(
            validation["generation_inputs"]["gate_revise"]["consumed_baseline"][kind]["revision"],
            2,
            "validation must consume the completed sibling rather than its historical baseline"
        );
    }
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 6);
    for prompt in &prompts {
        assert!(prompt.contains("Supplementary source answer: start only with explicit intent."));
        if prompt.contains("\nDocument: GATE_1\n") {
            assert!(
                !prompt.contains("Planning resolution:"),
                "judge contract stays unchanged"
            );
        } else {
            assert!(prompt.contains("standing (source-stated, derived, or proposed)"));
            assert!(prompt.contains("never invent measurements"));
        }
    }
}

#[test]
fn repeated_presses_keep_baselines_flat_and_checkpoint_records_bounded() {
    let (endpoint, server) = gateway(12, vec![FAIL_83, FAIL_83, FAIL_83, FAIL_83]);
    let mut engine = Engine::start("flat-baseline", &endpoint);
    let owner = engine.fixture(FAIL_81);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    // Four presses: two rounds of the first call, then two of a new call.
    for _ in 0..4 {
        engine.ask("qroc-gate-revise-start", args.clone());
        assert_eq!(
            engine.revise_until_done(&owner)["latest"]["state"],
            "failed"
        );
    }
    assert_eq!(
        engine.ask("qroc-gate-revise-get", args.clone())["record"]["calls"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let jobs = engine.ask("qroc-job-list", owner);
    for job in jobs["jobs"].as_array().unwrap() {
        assert!(serde_json::to_vec(job).unwrap().len() < 200_000);
        for baseline in job["gate_revise"]["baseline"].as_object().unwrap().values() {
            assert_eq!(baseline["generation_inputs"], Value::Null);
        }
    }
    engine.stop();
    assert_eq!(server.join().unwrap().len(), 12);
}

#[test]
fn oversized_baseline_is_refused_without_a_model_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut engine = Engine::start(
        "baseline-bound",
        &listener.local_addr().unwrap().to_string(),
    );
    let owner = engine.fixture(FAIL_81);
    let mut save = owner.clone();
    save["document_type"] = json!("EPICS");
    save["source_revision"] = json!(1);
    save["content"] = json!("x".repeat(1_100_000));
    engine.ask("qroc-document-save", save);
    let mut args = owner;
    args["gate"] = json!("GATE_3");
    let refusal = engine.reply("qroc-gate-revise-start", args);
    assert_eq!(refusal["ok"], false);
    assert_eq!(
        refusal["error"],
        "Revise baseline exceeds bounded context; documents retained"
    );
    engine.stop();
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err());
}

#[test]
fn verdict_whose_criteria_differ_from_the_rubric_never_passes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut engine = Engine::start(
        "criterion-identity",
        &listener.local_addr().unwrap().to_string(),
    );
    let owner = engine.fixture(PASS_92);
    let full = "- Problem and Users: 15/15 - ok\n- Requirement Testability: 25/25 - ok\n- Risk Coverage: 15/15 - ok\n- Validation Readiness: 20/20 - ok\n- Architecture Fit: 15/15 - ok\n";
    let cases = [
        // Consistency left out, the other five at full marks, total 100 (contract section 1).
        (
            format!("[SCORE]: 100\n{full}"),
            Some("it leaves out cross-document consistency"),
        ),
        (
            format!(
                "[SCORE]: 100\n{full}- Cross-Document Consistency: 10/10 - ok\n- Charm: 5/5 - ok\n"
            ),
            Some("it scores charm, which the rubric does not name"),
        ),
        (
            format!(
                "[SCORE]: 100\n{}- Cross-Document Consistency: 20/20 - ok\n",
                full.replace(
                    "Requirement Testability: 25/25",
                    "Requirement Testability: 15/15"
                )
            ),
            Some("requirement testability is scored out of 15 instead of 25"),
        ),
        (
            format!("[SCORE]: 100\n{full}- Cross-Document Consistency: 10/10 - ok\n"),
            None,
        ),
    ];
    for (n, (content, refusal)) in cases.iter().enumerate() {
        let mut save = owner.clone();
        save["document_type"] = json!("GATE_1");
        save["content"] = json!(content);
        if n > 0 {
            save["source_revision"] = json!(n);
        }
        engine.ask("qroc-document-save", save);
        let mut args = owner.clone();
        args["document_type"] = json!("GATE_1");
        args["revision"] = json!(n + 1);
        let verdict = engine.ask("qroc-gate-evaluate", args);
        match refusal {
            Some(words) => {
                assert_eq!(verdict["status"], "failed", "{verdict}");
                let reason = verdict["reason"].as_str().unwrap();
                assert!(reason.contains("do not match its rubric"), "{reason}");
                assert!(reason.contains(words), "{reason}");
            }
            None => assert_eq!(verdict["status"], "passed", "{verdict}"),
        }
    }
    engine.stop();
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "no request reached the gateway");
}

/// Gate 2 at 80 of 100 with two cross-document conflicts, each repair owned by one document.
fn gate_two_with_two_conflicts() -> String {
    let rubric = qroc_standalone::gate_rubric::rubric("GATE_2")
        .unwrap()
        .unwrap();
    let mut content = "[SCORE]: 80\n".to_owned();
    for c in &rubric.criteria {
        content.push_str(&format!(
            "- {}: {}/{} - fixture criterion feedback\n",
            c.name,
            c.max * 80 / 100,
            c.max
        ));
    }
    let consistency = &rubric
        .criteria
        .iter()
        .find(|c| c.name.contains("Consistency"))
        .expect("Gate 2 has a consistency criterion")
        .name;
    content.push_str(&format!(
        "\n## Required Remediation\n- [REPAIR] IMPLEMENTATION_PLAN | {consistency} | Reject whitespace-only notes, as DESIGN_ALTERNATIVES does.\n- [REPAIR] UI_UX_SPEC | {consistency} | Serve on port 38051, as ROADMAP states.\n"
    ));
    content
}

fn gate_two_pass() -> &'static str {
    let rubric = qroc_standalone::gate_rubric::rubric("GATE_2")
        .unwrap()
        .unwrap();
    let mut content = "[SCORE]: 100\n".to_owned();
    for c in rubric.criteria {
        content.push_str(&format!("- {}: {}/{} - consistent\n", c.name, c.max, c.max));
    }
    Box::leak(content.into_boxed_str())
}

#[test]
fn revise_round_runs_one_chain_and_each_owner_gets_its_repair_line() {
    const ORDER: [&str; 4] = [
        "DESIGN_ALTERNATIVES",
        "ROADMAP",
        "UI_UX_SPEC",
        "IMPLEMENTATION_PLAN",
    ];
    let (endpoint, server) = gateway(5, vec![gate_two_pass()]);
    let mut engine = Engine::start("gate-two-chain", &endpoint);
    let owner = engine.fixture(FAIL_81);
    let mut args = owner.clone();
    args["document_type"] = json!("GATE_2");
    args["content"] = json!(gate_two_with_two_conflicts());
    engine.ask("qroc-document-save", args.clone());
    args["revision"] = json!(1);
    let verdict = engine.ask("qroc-gate-evaluate", args);
    assert_eq!(verdict["status"], "failed");
    assert_eq!(verdict["repairs"].as_array().unwrap().len(), 2);
    let mut revise = owner.clone();
    revise["gate"] = json!("GATE_2");
    engine.ask("qroc-gate-revise-start", revise.clone());
    let mut done = Value::Null;
    for _ in 0..1000 {
        done = engine.ask("qroc-gate-revise-get", revise.clone());
        if done["running"] == false {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let round = &done["latest"]["rounds"][0];
    assert_eq!(done["latest"]["state"], "passed", "{done}");
    let documents = round["documents"].as_array().unwrap();
    let order = documents
        .iter()
        .map(|d| d["document_type"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(order, ORDER);
    // The round's record names which repair each writer received.
    let owned = |kind: &str| {
        documents
            .iter()
            .find(|d| d["document_type"] == kind)
            .unwrap()["repairs"]
            .as_array()
            .unwrap()
            .clone()
    };
    assert!(owned("DESIGN_ALTERNATIVES").is_empty());
    assert!(owned("ROADMAP").is_empty());
    assert_eq!(owned("UI_UX_SPEC").len(), 1);
    assert!(
        owned("UI_UX_SPEC")[0]["repair"]
            .as_str()
            .unwrap()
            .contains("port 38051")
    );
    assert_eq!(owned("IMPLEMENTATION_PLAN").len(), 1);
    assert!(
        owned("IMPLEMENTATION_PLAN")[0]["repair"]
            .as_str()
            .unwrap()
            .contains("whitespace-only notes")
    );
    let detail = |engine: &mut Engine, kind: &str, revision: u64| {
        let mut a = owner.clone();
        a["document_type"] = json!(kind);
        a["revision"] = json!(revision);
        engine.ask("qroc-document-detail", a)
    };
    let mut revised = std::collections::BTreeMap::new();
    for kind in ORDER {
        revised.insert(kind, detail(&mut engine, kind, 2));
    }
    // Every writer consumed every sibling at its then-current revision: the ones before
    // it in the chain at their new revision 2, the ones after it still at revision 1.
    for (position, kind) in ORDER.iter().enumerate() {
        let inputs = &revised[kind]["generation_inputs"];
        for (other_position, other) in ORDER.iter().enumerate() {
            if other == kind {
                continue;
            }
            let bound = if inputs["consumed_documents"][*other].is_object() {
                &inputs["consumed_documents"][*other]
            } else {
                &inputs["gate_revise"]["consumed_baseline"][*other]
            };
            let expected = if other_position < position {
                &revised[other]
            } else {
                &detail(&mut engine, other, 1)
            };
            assert_eq!(
                bound["revision"], expected["revision"],
                "{kind} reads {other}"
            );
            assert_eq!(bound["sha256"], expected["sha256"], "{kind} reads {other}");
            assert_eq!(bound["id"], expected["id"], "{kind} reads {other}");
        }
    }
    // The second writer reads the first's new revision in its prompt, not the old one.
    let roadmap_context = revised["ROADMAP"]["generation_inputs"]["context"]
        .as_str()
        .unwrap();
    assert!(roadmap_context.contains(&format!(
        "Revision baseline DESIGN_ALTERNATIVES revision 2 id {}",
        revised["DESIGN_ALTERNATIVES"]["id"].as_str().unwrap()
    )));
    assert!(!roadmap_context.contains("Revision baseline DESIGN_ALTERNATIVES revision 1"));
    let gate = detail(&mut engine, "GATE_2", 2);
    for kind in ORDER {
        assert_eq!(
            gate["generation_inputs"]["consumed_documents"][kind]["sha256"],
            revised[kind]["sha256"]
        );
    }
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 5);
    for (prompt, kind) in prompts.iter().zip(ORDER) {
        assert!(
            prompt.contains(&format!("\nDocument: {kind}\n")),
            "{kind} in chain order"
        );
        assert!(!prompt.contains("historical drafts"));
        assert!(!prompt.contains("not current sibling citations"));
        assert!(prompt.contains("revised one after another from one shared, current baseline"));
        let owns = format!("Repairs the gate assigned to {kind}.");
        match kind {
            "UI_UX_SPEC" => {
                assert!(prompt.contains(&owns));
                assert!(prompt.contains("- cross-document consistency: Serve on port 38051"));
                assert!(prompt.contains("- IMPLEMENTATION_PLAN (cross-document consistency, revised later in this round): Reject whitespace-only notes"));
            }
            "IMPLEMENTATION_PLAN" => {
                assert!(prompt.contains(&owns));
                assert!(
                    prompt.contains("- cross-document consistency: Reject whitespace-only notes")
                );
                assert!(prompt.contains("- UI_UX_SPEC (cross-document consistency, revised earlier in this round; its current revision is supplied): Serve on port 38051"));
            }
            _ => {
                assert!(!prompt.contains(&owns));
                assert!(prompt.contains("Repairs the gate assigned to other documents."));
            }
        }
    }
    assert!(prompts[4].contains("\nDocument: GATE_2\n"));
}

const HELD_88_REWORDED: &str = "[SCORE]: 88\n- Epic Coverage: 30/30 - every flow has an epic\n- Slice Independence: 25/25 - slices stand alone\n- Slice Acceptance: 25/25 - acceptance is measurable\n- Traceability: 8/20 - slices cannot name the phone they serve\n\n## Blocking Gaps\n1. [QUESTION] Traceability | SLICE_INDEX | Which phone model and operating system version will the first release use?\n\n## Required Remediation\n1. None a writer can make without that answer.";

#[test]
fn never_revised_gate_takes_his_answer_by_its_words_and_the_next_round_reads_it() {
    let (endpoint, server) = gateway(3, vec![PASS_92]);
    let mut engine = Engine::start("question-never-revised", &endpoint);
    let owner = engine.fixture(HELD_88);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    // No press yet: the gate keeps no question record.
    assert_eq!(
        engine.ask("qroc-gate-revise-get", args.clone())["questions"],
        json!([])
    );
    let mut answer = args.clone();
    answer["question"] = json!("Which phone model and system version will the first version use?");
    answer["answer"] = json!("A Pixel 9 on Android 16.");
    let settled = engine.ask("qroc-gate-question-answer", answer.clone());
    assert_eq!(settled["question"]["id"], "question-1");
    assert_eq!(settled["question"]["standing"], "answered");
    assert_eq!(settled["open_questions"], 0);
    answer["question"] = json!("A question the gate never asked?");
    assert_eq!(
        engine.reply("qroc-gate-question-answer", answer)["ok"],
        false
    );
    engine.ask("qroc-gate-revise-start", args);
    let done = engine.revise_until_done(&owner);
    assert_eq!(done["latest"]["state"], "passed");
    engine.stop();
    let prompts = server.join().unwrap();
    assert_eq!(prompts.len(), 3);
    assert!(prompts[0].contains("A Pixel 9 on Android 16."));
}

#[test]
fn an_earlier_wording_is_superseded_by_the_newer_and_no_longer_holds_the_gate() {
    let (endpoint, server) = gateway(0, vec![]);
    let mut engine = Engine::start("question-superseded", &endpoint);
    let owner = engine.fixture(HELD_88);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    // A refused press stores revision 1's question.
    assert_eq!(
        engine.reply("qroc-gate-revise-start", args.clone())["ok"],
        false
    );
    let mut gate = owner.clone();
    gate["document_type"] = json!("GATE_3");
    gate["source_revision"] = json!(1);
    gate["content"] = json!(HELD_88_REWORDED);
    engine.ask("qroc-document-save", gate);
    let mut answer = args.clone();
    answer["question"] =
        json!("Which phone model and operating system version will the first release use?");
    answer["answer"] = json!("A Pixel 9 on Android 16.");
    let settled = engine.ask("qroc-gate-question-answer", answer);
    assert_eq!(settled["question"]["id"], "question-2");
    assert_eq!(settled["open_questions"], 0);
    let state = engine.ask("qroc-gate-revise-get", args);
    let first = &state["questions"][0];
    assert_eq!(first["standing"], "superseded");
    assert_eq!(first["superseded_by"], "question-2");
    assert_eq!(
        first["question"],
        "Which phone model and system version will the first version use?"
    );
    assert_eq!(first["answer"], Value::Null);
    assert_eq!(state["open_questions"], 0);
    engine.stop();
    assert!(server.join().unwrap().is_empty());
}

/// Answers in order: each reply is (content, finish reason); records each request's prompt and
/// its max_tokens.
fn scripted(
    replies: Vec<(&'static str, &'static str)>,
) -> (String, thread::JoinHandle<Vec<(String, u64)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let mut seen = Vec::new();
        for (content, finish) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.strip_prefix("Content-Length: ") {
                    length = n.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let body = json!({"model":"test","choices":[{"message":{"content":content},"finish_reason":finish}]}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            seen.push((
                request["messages"][1]["content"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                request["max_tokens"].as_u64().unwrap(),
            ));
        }
        seen
    });
    (endpoint, server)
}

#[test]
fn a_writer_cut_at_its_limit_is_issued_again_at_the_risen_limit_and_its_partial_is_kept() {
    const CUT: &str = "Epics cut short by the output limit";
    let (endpoint, server) = scripted(vec![
        (CUT, "length"),
        ("Complete epics r3.", "stop"),
        ("Complete slices r2.", "stop"),
        (PASS_92, "stop"),
    ]);
    let mut engine = Engine::open_at(root("length-resume"), &endpoint, "512");
    let owner = engine.fixture(FAIL_81);
    engine.evaluate(&owner, 1);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    engine.ask("qroc-gate-revise-start", args.clone());
    let blocked = engine.revise_until_done(&owner);
    assert_eq!(blocked["latest"]["state"], "blocked", "{blocked}");
    // The partial draft is kept as a revision of its own, marked as cut short.
    let mut epics = owner.clone();
    epics["document_type"] = json!("EPICS");
    epics["revision"] = json!(2);
    let partial = engine.ask("qroc-document-detail", epics.clone());
    assert_eq!(partial["content"], CUT);
    assert_eq!(partial["generation_finish_reason"], "length");
    // The same limit would only cut it again: refused in words, nothing sent.
    let same = engine.reply("qroc-gate-revise-start", args.clone());
    assert_eq!(same["ok"], false);
    assert_eq!(
        same["error"],
        "EPICS stopped at its output limit of 512 and the route's limit is still 512; raise the route's limit before resuming"
    );
    let root = engine.root.clone();
    engine.stop();
    let mut engine = Engine::open_at(root, &endpoint, "1024");
    let resumed = engine.ask("qroc-gate-revise-start", args.clone());
    assert_eq!(resumed["resumed"], true);
    let done = engine.revise_until_done(&owner);
    assert_eq!(done["latest"]["state"], "passed", "{done}");
    epics["revision"] = json!(3);
    let complete = engine.ask("qroc-document-detail", epics.clone());
    assert_eq!(complete["content"], "Complete epics r3.");
    assert_eq!(complete["generation_inputs"]["max_tokens"], 1024);
    // The partial is still on disk as evidence, unchanged.
    epics["revision"] = json!(2);
    assert_eq!(engine.ask("qroc-document-detail", epics)["content"], CUT);
    // The slices writer and the gate read the complete epics, never the partial.
    let mut slices = owner.clone();
    slices["document_type"] = json!("SLICE_INDEX");
    slices["revision"] = json!(2);
    let slices = engine.ask("qroc-document-detail", slices);
    assert_eq!(
        slices["generation_inputs"]["consumed_documents"]["EPICS"]["revision"],
        3
    );
    let jobs = engine.ask("qroc-job-list", owner.clone());
    let epics_job = jobs["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["document_type"] == "EPICS")
        .unwrap()
        .clone();
    assert_eq!(epics_job["max_tokens"], 1024);
    assert_eq!(epics_job["attempts"][0]["max_tokens"], 512);
    assert_eq!(epics_job["attempts"][0]["state"], "partial");
    engine.stop();
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 4, "the refused press sent nothing");
    assert_eq!(seen[0].1, 512);
    for (prompt, max_tokens) in &seen[1..] {
        assert_eq!(*max_tokens, 1024);
        assert!(
            !prompt.contains(CUT),
            "no writer or judge reads the partial"
        );
    }
}

/// Like `scripted`, but each reply also says the output limit the route honoured.
fn scripted_honouring(
    replies: Vec<(&'static str, &'static str, u64)>,
) -> (String, thread::JoinHandle<Vec<u64>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let server = thread::spawn(move || {
        let mut asked = Vec::new();
        for (content, finish, honoured) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(n) = line.strip_prefix("Content-Length: ") {
                    length = n.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            let body = json!({"model":"test","choices":[{"message":{"content":content},"finish_reason":finish}],"honoured_max_tokens":honoured}).to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
            asked.push(request["max_tokens"].as_u64().unwrap());
        }
        asked
    });
    (endpoint, server)
}

#[test]
fn a_writer_cut_at_the_limit_the_route_honoured_is_issued_again_once_per_real_limit() {
    // The route honours 512 of the 1024 asked: the real limit is 512.
    let (endpoint, server) = scripted_honouring(vec![
        ("Epics cut at the honoured limit", "length", 512),
        ("Epics cut again at the same honoured limit", "length", 512),
    ]);
    let root = root("honoured-limit");
    let mut engine = Engine::open_at(root.clone(), &endpoint, "512");
    let owner = engine.fixture(FAIL_81);
    engine.evaluate(&owner, 1);
    let mut args = owner.clone();
    args["gate"] = json!("GATE_3");
    let root = engine.root.clone();
    engine.stop();
    // Asked 1024, honoured 512.
    let mut engine = Engine::open_at(root.clone(), &endpoint, "1024");
    engine.ask("qroc-gate-revise-start", args.clone());
    assert_eq!(
        engine.revise_until_done(&owner)["latest"]["state"],
        "blocked"
    );
    let jobs = engine.ask("qroc-job-list", owner.clone());
    let epics = |jobs: &Value| {
        jobs["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["document_type"] == "EPICS")
            .unwrap()
            .clone()
    };
    assert_eq!(epics(&jobs)["max_tokens"], 1024);
    assert_eq!(epics(&jobs)["honoured_max_tokens"], 512);
    engine.stop();
    // At the same asked limit, the real limit of 512 is below it: one re-issue is allowed.
    let mut engine = Engine::open_at(root.clone(), &endpoint, "1024");
    let resumed = engine.ask("qroc-gate-revise-start", args.clone());
    assert_eq!(resumed["resumed"], true);
    assert_eq!(
        engine.revise_until_done(&owner)["latest"]["state"],
        "blocked"
    );
    // Cut again at the same honoured limit: a second re-issue is refused in words.
    let again = engine.reply("qroc-gate-revise-start", args);
    assert_eq!(again["ok"], false);
    assert_eq!(
        again["error"],
        "EPICS was already issued again and stopped again at the route's real limit of 512; the route honours no more than that"
    );
    let job = epics(&engine.ask("qroc-job-list", owner.clone()));
    assert_eq!(job["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(job["attempts"][0]["honoured_max_tokens"], 512);
    engine.stop();
    assert_eq!(
        server.join().unwrap(),
        [1024, 1024],
        "two requests, the refused press sent none"
    );
}
