//! QROC's money guard lives in the shared API layer (Stratum, `/example/operator/80_stratum/tools/api-layer`),
//! lifted there from this file on 30 Oct 2026: before a request leaves, the layer reserves a conservative
//! upper bound of its charge and admits it only when it fits QROC's cap in the layer's `caps.json` and this
//! batch's own ceiling (`--ceiling`, kept in this state folder's `money.json` as before: the tighter stop
//! wins); after the reply it settles on the billed usage x the dated price of the served model, in both
//! accounts and in the layer's one ledger. An unknown charge stays reserved, and while one is unresolved
//! nothing more is admitted; `route-shim reconcile` settles it with evidence. This module is QROC's view.
use std::path::{Path, PathBuf};
pub use stratum_api::money::{Billing, input_bound, load_ceiling};
#[cfg(test)]
pub use stratum_api::prices::{Price, charge, per_token, reserve};
pub use stratum_api::prices::{amount, fmt};

/// QROC's name in the layer: its cap, its ledger lines, its spend.
pub const TOOL: &str = "qroc";

/// The two files the guard reads on every request: the price table (the layer's own, named by
/// `--prices`) and this batch's ceiling.
pub struct Files {
    pub prices: PathBuf,
    pub ceiling: PathBuf,
}

/// The shared layer, at its installed roots (or the test roots in `STRATUM_API_HOME`/`STRATUM_API_CONFIG`).
pub fn layer() -> Result<stratum_api::Layer, String> {
    stratum_api::Layer::open().map_err(|e| e.to_string())
}

/// `--prices` must name the layer's one price table (the same file, links followed).
pub fn check_prices(layer: &stratum_api::Layer, prices: &Path) -> Result<(), String> {
    let ours = std::fs::canonicalize(layer.prices_path())
        .map_err(|e| format!("the shared price table is unreadable: {e}"))?;
    let named = std::fs::canonicalize(prices).map_err(|e| format!("price file unreadable: {e}"))?;
    if ours != named {
        return Err(format!(
            "--prices must name the shared price table {}",
            layer.prices_path().display()
        ));
    }
    Ok(())
}

/// Settle a reservation of this batch: through the layer when it holds it (both accounts and the
/// ledger), else (one admitted before the layer) in this folder only. Returns settled and reserved here.
pub fn reconcile(
    state: &Path,
    id: &str,
    charge: u64,
    evidence: &str,
) -> Result<(u64, u64), String> {
    let layer = layer()?;
    if stratum_api::money::holds(&layer, TOOL, id) {
        layer
            .reconcile(TOOL, id, charge, evidence)
            .map_err(|e| e.to_string())?;
        return stratum_api::money::inner_figures(state);
    }
    stratum_api::money::reconcile_inner(state, id, charge, evidence)
}

/// One line for `status`: settled, reserved and unresolved, never a guess.
pub fn summary(state: &Path) -> String {
    stratum_api::money::inner_summary(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn decimals_are_exact() {
        assert_eq!(per_token(&json!("2.50")), Some(2500));
        assert_eq!(per_token(&json!("0.10")), Some(100));
        assert_eq!(per_token(&json!("4")), Some(4000));
        assert_eq!(
            per_token(&json!("0.0001")),
            None,
            "finer than nano per token is refused"
        );
        assert_eq!(amount(&json!("1.000000001")), Some(1_000_000_001));
        assert_eq!(amount(&json!(-1)), None);
        assert_eq!(fmt(1_000_000_001), "1.000000001");
    }
    #[test]
    fn openai_charge_splits_cached_and_long_context() {
        let p = Price {
            key: "openai:m".into(),
            model: "m".into(),
            input: 2000,
            cached_input: 100,
            cache_write_5m: 2000,
            cache_write_1h: 2000,
            output: 10000,
            long: Some((272000, 4000, 200, 15000)),
            ..Default::default()
        };
        let r = json!({"usage":{"prompt_tokens":1000,"completion_tokens":10,"prompt_tokens_details":{"cached_tokens":400}}});
        assert_eq!(
            charge("openai", &p, &r),
            Ok(600 * 2000 + 400 * 100 + 10 * 10000)
        );
        let r = json!({"usage":{"prompt_tokens":300000,"completion_tokens":1}});
        assert_eq!(charge("openai", &p, &r), Ok(300000 * 4000 + 15000));
        assert!(charge("openai", &p, &json!({"usage":{}})).is_err());
        assert!(
            charge(
                "openai",
                &p,
                &json!({"service_tier":"flex","usage":{"prompt_tokens":1,"completion_tokens":1}})
            )
            .is_err()
        );
        assert_eq!(reserve(&p, 300001, 10), Some(300001 * 4000 + 10 * 15000));
    }
}
