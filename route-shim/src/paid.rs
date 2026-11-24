//! Transport of paid routes, through the shared API layer (`stratum_api::chat`, lifted from here on
//! 30 Oct 2026): OpenAI's chat completions (`apikey`) and Anthropic's Messages API (`anthropic`), the
//! key read by the layer from its one store and written to curl's standard input, never on a command
//! line, the reply streamed and classed so the guard can tell a refused request from an unknown charge.
//! This shim keeps no key and reads no key variable for these routes.
use crate::codex::{Failure, ResultData, disconnected};
use crate::money::Billing;
use serde_json::Value;
use std::{net::TcpStream, path::Path};

pub fn provider(r: &Value) -> &'static str {
    if r["kind"] == "anthropic" {
        "anthropic"
    } else {
        "openai"
    }
}

/// An `apikey` route to a provider rather than to a local stand-in on loopback is paid, priced or not.
pub fn paid(r: &Value) -> bool {
    match r["kind"].as_str() {
        Some("anthropic") => true,
        Some("apikey") => {
            crate::routes::priced(r)
                || !r["base"]
                    .as_str()
                    .and_then(|b| b.strip_prefix("http://"))
                    .and_then(|h| h.split('/').next())
                    .and_then(|h| h.parse::<std::net::SocketAddr>().ok())
                    .is_some_and(|a| a.ip().is_loopback())
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn invoke(
    layer: &stratum_api::Layer,
    r: &Value,
    body: &Value,
    max_output: u64,
    state: &Path,
    id: &str,
    client: &TcpStream,
    timeout: u64,
) -> (Result<ResultData, Failure>, Billing) {
    let call = stratum_api::chat::Call {
        id,
        provider: provider(r),
        base: r["base"].as_str().unwrap_or_default(),
        model: r["model"].as_str().unwrap_or_default(),
        effort: r["effort"].as_str(),
        messages: &body["messages"],
        max_output,
        timeout_ms: timeout,
        keep_dir: Some(&state.join("failed")),
    };
    let out = stratum_api::chat::complete(layer, &call, &|| disconnected(client));
    let reply = match out.reply {
        Ok(value) => Ok(ResultData {
            value,
            exit: Some(0),
            honoured: true,
            hash: None,
        }),
        Err(status) => Err(Failure { status, exit: None }),
    };
    (reply, out.billing)
}
