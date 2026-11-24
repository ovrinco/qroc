// Unchanged call site extracted from QROC release qroc-20261030-credit-d02.
// Source: release_sources/api_layer_w7/src/routes.rs
// Whole-source SHA-256: 07fa5831ade43363a1a52e5f9eb7205fdff9d6fc449546a17992d63ec98d6b92
pub const LAYER_OPENROUTER_KEY: &str = "OPENROUTER_API_KEY";
fn layer_has_key(env: &dyn Fn(&str) -> Option<String>) -> bool {
    let config = env("STRATUM_API_CONFIG")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config/stratum/api")));
    config.is_some_and(|c| stratum_api::keys::present(&c.join("keys.env"), LAYER_OPENROUTER_KEY))
}
