//! `stratum-api`: the shared API layer's command line, a thin front over the library.
use stratum_api::{Layer, caps, images, ledger, prices, serve};

const HELP: &str = "stratum-api: the shared API layer (one key store, one price table, a cap per tool, one ledger)

  stratum-api spend [--json]        what each tool has spent, in all and per day, against its cap
  stratum-api status [--json]       the same, with the settings files the layer reads
  stratum-api caps                  each tool's cap (a cap is changed by editing caps.json)
  stratum-api reconcile --tool TOOL --id ID --charge AMOUNT --evidence TEXT
                                    settle a call whose charge was unknown, with where the figure came from
  stratum-api image --tool TOOL (--prompt-file FILE | --prompt TEXT) --size WxH --quality Q --out DIR
                    [--n N] [--image REF ...] [--format png|jpeg|webp] [--background B] [--route LABEL] [--json]
                                    make images through the image route (with --image: an edit from those
                                    reference images); writes the files into DIR and prints the receipt and charge
  stratum-api serve [--listen ADDR] [--idle-secs N] [--timeout-ms N]
                                    the local service (normally started by its socket unit)";

fn arg(a: &[String], name: &str) -> Option<String> {
    a.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn main() {
    if let Err(e) = entry() {
        eprintln!("stratum-api: {e}");
        std::process::exit(2);
    }
}

fn entry() -> stratum_api::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let layer = Layer::open()?;
    let json = a.iter().any(|x| x == "--json");
    match a.get(1).map(String::as_str) {
        Some("spend") => {
            let s = layer.spend()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&s.to_json()).unwrap_or_default()
                );
            } else {
                print!("{}", s.to_text());
            }
        }
        Some("status") => {
            let s = layer.spend()?;
            let check = |p: &std::path::Path, ok: bool| {
                format!(
                    "{} ({})",
                    p.display(),
                    if ok { "readable" } else { "missing or invalid" }
                )
            };
            let prices_ok = prices::load(&layer.prices_path()).is_ok();
            let caps_ok = caps::load(&layer.caps_path()).is_ok();
            let routes_ok = stratum_api::chat::routes(&layer.routes_path()).is_ok();
            let key_service = layer.keysvc_socket()?;
            // Presence and last use per provider, never a value; None when the service is unreachable.
            let presence = key_service
                .as_ref()
                .and_then(|socket| stratum_api::keysvc::presence(socket).ok());
            let keys_ok = match &key_service {
                Some(_) => presence.is_some(),
                None => stratum_api::keys::check_owner_only(&layer.keys_path()).is_ok(),
            };
            if json {
                let mut v = s.to_json();
                v["settings"] = serde_json::json!({"prices":prices_ok,"caps":caps_ok,"routes":routes_ok,"key_store_owner_only":if key_service.is_none(){Some(keys_ok)}else{None},"key_service_reachable":key_service.as_ref().map(|_|keys_ok)});
                if let Some(p) = &presence {
                    v["settings"]["keys"] = p
                        .iter()
                        .map(|(name, set, last)| {
                            (
                                name.to_string(),
                                serde_json::json!({"set":set,"last_used":last}),
                            )
                        })
                        .collect::<serde_json::Map<_, _>>()
                        .into();
                }
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                println!("Price table: {}", check(&layer.prices_path(), prices_ok));
                println!("Caps: {}", check(&layer.caps_path(), caps_ok));
                println!("Routes: {}", check(&layer.routes_path(), routes_ok));
                if key_service.is_some() {
                    println!(
                        "Key service: {}",
                        if keys_ok { "available" } else { "unavailable" }
                    );
                    for (name, set, last) in presence.iter().flatten() {
                        match (set, last) {
                            (false, _) => println!("  {name}: not set"),
                            (true, None) => println!("  {name}: present, not used yet"),
                            (true, Some(t)) => println!(
                                "  {name}: present, last used {}",
                                ledger::local_minute(*t)
                            ),
                        }
                    }
                } else {
                    println!(
                        "Key store: {}",
                        if keys_ok {
                            "present and owner-only"
                        } else {
                            "missing or open to others"
                        }
                    );
                }
                println!("Ledger: {}", layer.home.join(ledger::LEDGER).display());
                print!("{}", s.to_text());
            }
        }
        Some("caps") => {
            let c = caps::load(&layer.caps_path()).map_err(stratum_api::Error::Invalid)?;
            for (tool, cap) in &c.tools {
                match cap.per_day {
                    Some(d) => println!(
                        "{tool}: {} {} in all, at most {} a day",
                        prices::plain(cap.total),
                        c.currency,
                        prices::plain(d)
                    ),
                    None => println!("{tool}: {} {} in all", prices::plain(cap.total), c.currency),
                }
            }
            println!(
                "A tool not listed here is refused. Change a cap by editing {}.",
                layer.caps_path().display()
            );
        }
        Some("reconcile") => {
            let need = |n: &str| {
                arg(&a, n)
                    .ok_or_else(|| stratum_api::Error::Invalid(format!("reconcile needs {n}")))
            };
            let charge =
                prices::amount(&serde_json::json!(need("--charge")?)).ok_or_else(|| {
                    stratum_api::Error::Invalid(
                        "--charge must be an exact non-negative amount".into(),
                    )
                })?;
            let t = layer.reconcile(
                &need("--tool")?,
                &need("--id")?,
                charge,
                &need("--evidence")?,
            )?;
            println!(
                "Reconciled. The tool has now spent {} and holds {} for calls in flight or unknown.",
                prices::plain(t.settled),
                prices::plain(t.reserved)
            );
        }
        Some("image") => image(&layer, &a, json)?,
        Some("serve") => {
            let listen = arg(&a, "--listen").unwrap_or_else(|| serve::DEFAULT_LISTEN.into());
            let idle = arg(&a, "--idle-secs")
                .and_then(|n| n.parse().ok())
                .map(std::time::Duration::from_secs)
                .unwrap_or(serve::IDLE);
            let timeout = arg(&a, "--timeout-ms")
                .and_then(|n| n.parse().ok())
                .unwrap_or(600_000);
            let l = serve::listener(&listen)?;
            if let Ok(addr) = l.local_addr() {
                println!("LISTEN {addr}");
            }
            serve::run(layer, l, idle, timeout)?;
        }
        Some("-h" | "--help" | "help") | None => println!("{HELP}"),
        Some(other) => {
            return Err(stratum_api::Error::Invalid(format!(
                "unknown command '{other}'\n{HELP}"
            )));
        }
    }
    Ok(())
}

/// `stratum-api image`: one image request through the library's guard, the images written to files.
fn image(layer: &Layer, a: &[String], json: bool) -> stratum_api::Result<()> {
    let bad = |m: String| stratum_api::Error::Invalid(m);
    let need = |n: &str| arg(a, n).ok_or_else(|| bad(format!("image needs {n}")));
    let tool = need("--tool")?;
    let prompt = match (arg(a, "--prompt-file"), arg(a, "--prompt")) {
        (Some(f), None) => std::fs::read_to_string(&f).map_err(|e| bad(format!("{f}: {e}")))?,
        (None, Some(p)) => p,
        _ => return Err(bad("image needs one of --prompt-file or --prompt".into())),
    };
    let out = std::path::PathBuf::from(need("--out")?);
    let format = arg(a, "--format").unwrap_or_else(|| "png".into());
    let refs: Vec<&String> = a
        .windows(2)
        .filter(|w| w[0] == "--image")
        .map(|w| &w[1])
        .collect();
    let mut body = serde_json::json!({"prompt":prompt.trim_end(),"size":need("--size")?,"quality":need("--quality")?,"output_format":format});
    if let Some(n) = arg(a, "--n") {
        body["n"] = serde_json::json!(
            n.parse::<u64>()
                .map_err(|_| bad("--n must be a whole number".into()))?
        );
    }
    for k in ["background", "moderation"] {
        if let Some(v) = arg(a, &format!("--{k}")) {
            body[k] = serde_json::json!(v);
        }
    }
    if let Some(r) = arg(a, "--route") {
        body["model"] = serde_json::json!(r);
    }
    let kind = if refs.is_empty() {
        images::Kind::Generate
    } else {
        let list = refs
            .iter()
            .map(|p| {
                images::data_url(std::path::Path::new(p))
                    .map(|u| serde_json::json!({"image_url":u}))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(bad)?;
        body["images"] = serde_json::json!(list);
        images::Kind::Edit
    };
    std::fs::create_dir_all(&out).map_err(|e| bad(format!("{}: {e}", out.display())))?;
    match layer.image(&tool, kind, &body, 600_000) {
        Ok((reply, r)) => {
            let mut files = Vec::new();
            for (i, d) in reply["data"].as_array().into_iter().flatten().enumerate() {
                let bytes = d["b64_json"]
                    .as_str()
                    .and_then(images::b64_decode)
                    .ok_or_else(|| bad("an image in the reply could not be decoded".into()))?;
                let path = out.join(format!("{}-{}.{format}", r.id, i + 1));
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .and_then(|mut f| f.write_all(&bytes))
                    .map_err(|e| bad(format!("{}: {e}", path.display())))?;
                files.push(path.display().to_string());
            }
            if json {
                let mut v = r.to_json();
                v["files"] = serde_json::json!(files);
                v["usage"] = reply["usage"].clone();
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                println!(
                    "Receipt {}: charged {} {} (reserved {} before sending), {}.",
                    r.id,
                    r.charge
                        .map(prices::plain)
                        .unwrap_or_else(|| "an unknown amount".into()),
                    r.currency,
                    prices::plain(r.reservation),
                    r.basis
                );
                for f in &files {
                    println!("Wrote {f}");
                }
            }
            Ok(())
        }
        Err(f) => {
            let mut m = f.error.clone();
            if let Some(said) = f.detail["provider_message"].as_str() {
                m.push_str(&format!("; the provider said: {said}"));
            }
            if let Some(r) = &f.receipt {
                m.push_str(&format!(
                    " (receipt {}: charged {})",
                    r.id,
                    r.charge
                        .map(prices::plain)
                        .unwrap_or_else(|| "an unknown amount, held until reconciled".into())
                ));
            } else {
                m.push_str("; nothing was sent");
            }
            Err(bad(m))
        }
    }
}
