//! The guard of the shared layer on a fake provider (loopback only; no real provider, no paid call):
//! a call over its tool's cap is refused unsent, an unknown charge stays reserved and blocks that tool
//! until reconciled, per-day caps and inner ceilings stop as well, both providers are priced, settled
//! and logged in the one ledger, and no key value lands in any file.
mod fake;
use fake::{Root, fake, root};
use serde_json::{Value, json};
use stratum_api::{Admitted, Ask, Inner, Refusal, RefusalKind, Settled, chat, money};

// One user message "x": bound 512 + 32 + 4 + 1 = 549 tokens. Anthropic: 549 x 8000 (1h cache write)
// + 100 x 20000 = 6,392,000 nano. OpenAI: 549 x 2000 + 100 x 10000 = 2,098,000 nano.
const ANTHROPIC_ONE: u64 = 6_392_000;
const OPENAI_ONE: u64 = 2_098_000;

struct Done {
    admitted: Result<Admitted, Refusal>,
    settled: Option<Settled>,
    reply: Option<Result<Value, u16>>,
}

fn call(r: &Root, tool: &str, label: &str, content: &str, inner: Option<&Inner>) -> Done {
    let layer = r.layer();
    let route = layer.route(label).unwrap();
    let messages = json!([{"role":"user","content":content}]);
    let fp = money::fingerprint(label, &messages, route.max_output);
    let ask = Ask {
        tool,
        id: None,
        route: label,
        provider: &route.provider,
        model: &route.model,
        input_bound: money::input_bound(&messages),
        max_output: route.max_output,
        fingerprint: &fp,
        inner,
    };
    let admitted = layer.admit(&ask);
    let Ok(a) = &admitted else {
        return Done {
            admitted,
            settled: None,
            reply: None,
        };
    };
    let c = chat::Call {
        id: &a.id,
        provider: &route.provider,
        base: &route.base,
        model: &route.model,
        effort: route.effort.as_deref(),
        messages: &messages,
        max_output: route.max_output,
        timeout_ms: 3000,
        keep_dir: None,
    };
    let out = chat::complete(&layer, &c, &|| false);
    let settled = layer.settle(a, out.billing);
    Done {
        admitted,
        settled: Some(settled),
        reply: Some(out.reply),
    }
}

fn usd(nano: u64) -> String {
    format!("{}.{:09}", nano / 1_000_000_000, nano % 1_000_000_000)
}

#[test]
fn over_cap_refused_unsent() {
    let f = fake();
    let r = root("cap", &f);
    r.caps(json!({"qroc":{"total":usd(ANTHROPIC_ONE - 1)}}));
    let d = call(&r, "qroc", "anthropic:fake-claude", "x", None);
    let e = d.admitted.unwrap_err();
    assert_eq!(e.kind, RefusalKind::Cap, "{e}");
    assert_eq!(e.status(), 429);
    assert_eq!(f.hits(), 0, "a refused call never reaches the provider");
    r.caps(json!({"qroc":{"total":usd(ANTHROPIC_ONE)}}));
    let d = call(&r, "qroc", "anthropic:fake-claude", "x", None);
    assert!(d.admitted.is_ok(), "equal to the cap fits");
    assert_eq!(f.hits(), 1);
    // 10 x 4000 + 5 x 20000 = 140,000 settled; the next reservation does not fit.
    assert_eq!(d.settled.unwrap().charge, Some(140_000));
    let d = call(&r, "qroc", "anthropic:fake-claude", "x", None);
    assert_eq!(d.admitted.unwrap_err().kind, RefusalKind::Cap);
    assert_eq!(f.hits(), 1, "still one hit after the second refusal");
    // A tool with no cap is refused; so is another tool, unaffected by qroc's spend.
    let d = call(&r, "qream", "openai:fake-gpt", "x", None);
    assert_eq!(d.admitted.unwrap_err().kind, RefusalKind::NoCap);
    assert_eq!(f.hits(), 1);
    let refusals = r
        .ledger()
        .iter()
        .filter(|l| l["decision"] == "refuse")
        .count();
    assert_eq!(refusals, 3, "each refusal is a ledger line");
    println!(
        "cap: one nano below refused unsent, equal admitted, next over refused, uncapped tool refused (provider hits 1 of 4)"
    );
}

#[test]
fn per_day_cap_and_inner_ceiling_stop() {
    let f = fake();
    let r = root("day", &f);
    // zeno: the day holds one call and its settled charge, not a second reservation.
    r.caps(
        json!({"qroc":{"total":"1"},"zeno":{"total":"1","per_day":usd(OPENAI_ONE + 62_400 - 1)}}),
    );
    assert!(
        call(&r, "zeno", "openai:fake-gpt", "x", None)
            .admitted
            .is_ok()
    );
    let d = call(&r, "zeno", "openai:fake-gpt", "x", None);
    let e = d.admitted.unwrap_err();
    assert_eq!(e.kind, RefusalKind::Cap);
    assert!(e.reason.contains("daily cap"), "{e}");
    assert_eq!(f.hits(), 1);
    // An inner ceiling tighter than the tool's cap wins, in QROC's own form.
    let state = r.dir.join("inner-state");
    std::fs::create_dir_all(&state).unwrap();
    let ceiling = r.dir.join("CEILING.json");
    std::fs::write(
        &ceiling,
        json!({"ceiling":{"amount":usd(OPENAI_ONE - 1),"currency":"USD"}}).to_string(),
    )
    .unwrap();
    let inner = Inner {
        ceiling: ceiling.clone(),
        state: state.clone(),
    };
    let d = call(&r, "qroc", "openai:fake-gpt", "x", Some(&inner));
    let e = d.admitted.unwrap_err();
    assert_eq!(e.reason, "spending ceiling reached");
    assert_eq!(f.hits(), 1);
    std::fs::write(
        &ceiling,
        json!({"ceiling":{"amount":"1","currency":"USD"}}).to_string(),
    )
    .unwrap();
    let d = call(&r, "qroc", "openai:fake-gpt", "x", Some(&inner));
    let s = d.settled.unwrap();
    // 6 uncached x 2000 + 4 cached x 100 + 5 x 10000 = 62,400, on both the tool and its inner ceiling.
    assert_eq!(s.charge, Some(62_400));
    assert_eq!(s.inner.unwrap().settled, 62_400);
    let m: Value =
        serde_json::from_slice(&std::fs::read(state.join("money.json")).unwrap()).unwrap();
    assert_eq!(m["version"], "qroc-money/1");
    assert_eq!(m["settled_nano"], 62_400);
    assert_eq!(r.accounts()["tools"]["qroc"]["settled_nano"], 62_400);
    println!(
        "per day: second call past the day's cap refused unsent; inner ceiling tighter than the cap refused unsent; both accounts settled 0.0000624"
    );
}

#[test]
fn unknown_charge_stays_reserved_and_blocks_until_reconciled() {
    let f = fake();
    let r = root("unknown", &f);
    let layer = r.layer();
    for (mode, label, reserved) in [
        ("cut", "anthropic:fake-claude", ANTHROPIC_ONE),
        ("overloaded", "openai:fake-gpt", OPENAI_ONE),
        ("no-usage", "openai:fake-gpt", OPENAI_ONE),
    ] {
        f.set(mode);
        let d = call(&r, "qroc", label, "x", None);
        let a = d.admitted.unwrap();
        let s = d.settled.unwrap();
        assert_eq!(s.charge, None, "{mode}: {}", s.basis);
        assert!(s.basis.starts_with("unknown"), "{mode}: {}", s.basis);
        assert_eq!(
            s.tool.reserved, reserved,
            "{mode}: the reservation stays counted"
        );
        assert_eq!(s.tool.unresolved, 1);
        f.set("ok");
        let hits = f.hits();
        let e = call(&r, "qroc", label, "y", None).admitted.unwrap_err();
        assert_eq!(e.kind, RefusalKind::Unresolved, "{mode}");
        assert_eq!(
            f.hits(),
            hits,
            "{mode}: nothing admitted while a charge is unknown"
        );
        // Another tool is not blocked by qroc's unknown charge.
        assert!(call(&r, "zeno", label, "y", None).admitted.is_ok());
        layer
            .reconcile("qroc", &a.id, 100_000, "fixture: provider usage row")
            .unwrap();
        assert!(
            call(&r, "qroc", label, "y", None).admitted.is_ok(),
            "{mode}: admission resumes after reconciliation"
        );
    }
    // A refusal before generation (400) is not billed.
    f.set("reject");
    let s = call(&r, "qroc", "openai:fake-gpt", "z", None)
        .settled
        .unwrap();
    assert_eq!(s.charge, Some(0));
    assert!(s.basis.contains("HTTP 400"));
    let ledger = r.ledger();
    assert_eq!(
        ledger
            .iter()
            .filter(|l| l["decision"] == "reconcile")
            .count(),
        3
    );
    for l in &ledger {
        assert!(
            l["tool"].is_string() && l["date"].is_string() && l["id"].is_string(),
            "{l}"
        );
    }
    let files = fake::no_key_anywhere(&r.dir);
    println!(
        "unknown: cut stream, 529 and missing usage kept reserved and blocking (3 of 3) until reconciled; 400 not billed; no key in {files} files"
    );
}

#[test]
fn both_providers_priced_settled_and_logged() {
    let f = fake();
    let r = root("both", &f);
    let d = call(&r, "zeno", "anthropic:fake-claude", "x", None);
    let reply = d.reply.unwrap().unwrap();
    assert_eq!(reply["choices"][0]["message"]["content"], "answer");
    assert_eq!(d.settled.unwrap().charge, Some(140_000));
    let sent = f.seen.lock().unwrap()[0].clone();
    assert_eq!(sent.0, "/v1/messages");
    assert!(
        sent.1.contains("anthropic-version: 2026-10-21")
            && sent.1.to_ascii_lowercase().contains("x-api-key:")
    );
    assert_eq!(sent.2["max_tokens"], 100);
    assert_eq!(sent.2["output_config"]["effort"], "medium");
    assert_eq!(sent.2["stream"], true);
    let d = call(&r, "zeno", "openai:fake-gpt", "x", None);
    assert_eq!(d.settled.unwrap().charge, Some(62_400));
    let sent = f.seen.lock().unwrap()[1].clone();
    assert_eq!(sent.0, "/v1/chat/completions");
    assert_eq!(sent.2["max_completion_tokens"], 100);
    assert_eq!(sent.2["reasoning_effort"], "medium");
    let ledger = r.ledger();
    let order: Vec<&str> = ledger
        .iter()
        .map(|l| l["decision"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["admit", "settle", "admit", "settle"]);
    assert_eq!(ledger[1]["generation_id"], "msg_fixture");
    assert_eq!(ledger[1]["charge"], "0.000140000");
    assert_eq!(ledger[3]["served_model"], "fake-gpt-2026-10-01");
    let spend = r.layer().spend().unwrap();
    let z = spend.tools.iter().find(|t| t.tool == "zeno").unwrap();
    assert_eq!(z.total.settled, 202_400);
    assert_eq!(z.days.len(), 1);
    assert_eq!(z.days[0].1.settled, 202_400);
    let text = spend.to_text();
    assert!(text.contains("zeno: spent 0.0002024 USD"), "{text}");
    println!(
        "both providers: Anthropic stand-in and OpenAI fake priced, settled, ledger admit→settle x2, spend per tool and day 0.0002024"
    );
}

#[test]
fn key_store_must_be_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let f = fake();
    let r = root("keys", &f);
    let k = r.config().join("keys.env");
    std::fs::set_permissions(&k, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(r.layer().key("OPENAI_API_KEY").is_err());
    let d = call(&r, "qroc", "openai:fake-gpt", "x", None);
    let s = d.settled.unwrap();
    assert_eq!(s.charge, Some(0), "not sent: {}", s.basis);
    assert!(s.basis.starts_with("not sent"));
    assert_eq!(f.hits(), 0);
    std::fs::set_permissions(&k, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(r.config(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        r.layer().key("OPENAI_API_KEY").is_err(),
        "a folder open to others is refused too"
    );
    std::fs::set_permissions(r.config(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let key = r.layer().key("OPENAI_API_KEY").unwrap();
    assert_eq!(format!("{key:?}"), "Secret(***)");
    println!(
        "keys: store open to others refused (file and folder), call not sent; read once owner-only; printed masked"
    );
}
