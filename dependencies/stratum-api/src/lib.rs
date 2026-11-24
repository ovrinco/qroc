//! The shared API layer of the stack (Stratum). Every tool that calls a cloud provider does so through
//! this library: one owner-only key store, one dated price table, a spend guard with a cap per tool,
//! one ledger of what was admitted, refused and settled. Lifted from the route and guard QROC built on
//! 30 Oct 2026 (route-shim `money.rs`, `paid.rs`, `anthropic.rs`, `stream.rs`); the guard's rules are
//! QROC's, extended from one batch ceiling to one cap per tool.
//!
//! The flow of one call: [`Layer::admit`] (priced, checked against the tool's cap and an optional inner
//! ceiling, reserved and synced before anything leaves) → the request (through [`chat`], or a caller's
//! own wire format using [`chat::post`]) → [`Layer::settle`] with a [`Billing`] class.
use std::path::{Path, PathBuf};

pub mod anthropic;
pub mod caps;
pub mod chat;
pub mod images;
pub mod keys;
pub mod keysvc;
pub mod ledger;
pub mod money;
pub mod prices;
pub mod responses;
pub mod serve;
pub mod stream;

pub use keys::Secret;
pub use money::{Admitted, Ask, Billing, Inner, Refusal, RefusalKind, Settled};

/// One error type for the library; a refusal of the guard is not an error but a [`Refusal`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: not valid JSON")]
    Json { path: PathBuf },
    #[error("{0}")]
    Invalid(String),
    #[error("key store: {0}")]
    Keys(String),
    #[error("ledger: {0}")]
    Ledger(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io_at(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// The settings and state roots of the layer: settings (caps, prices, routes, keys) under
/// `~/.config/stratum/api`, state (ledger, accounts, lock) under `~/.local/share/stratum/api`.
/// `STRATUM_API_CONFIG` and `STRATUM_API_HOME` move them, for tests only.
#[derive(Debug, Clone)]
pub struct Layer {
    pub home: PathBuf,
    pub config: PathBuf,
}

impl Layer {
    pub fn open() -> Result<Layer> {
        let user = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Invalid("HOME is not set".into()))?;
        let home = std::env::var_os("STRATUM_API_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user.join(".local/share/stratum/api"));
        let config = std::env::var_os("STRATUM_API_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| user.join(".config/stratum/api"));
        Ok(Layer::at(home, config))
    }

    pub fn at(home: impl Into<PathBuf>, config: impl Into<PathBuf>) -> Layer {
        Layer {
            home: home.into(),
            config: config.into(),
        }
    }

    pub fn prices_path(&self) -> PathBuf {
        self.config.join("prices.json")
    }
    pub fn caps_path(&self) -> PathBuf {
        self.config.join("caps.json")
    }
    pub fn routes_path(&self) -> PathBuf {
        self.config.join("routes.json")
    }
    pub fn keys_path(&self) -> PathBuf {
        self.config.join("keys.env")
    }

    /// An explicit switch; malformed configuration fails closed, with no legacy fallback.
    pub fn keysvc_socket(&self) -> Result<Option<PathBuf>> {
        let path = self.config.join("keysvc.json");
        let exists = match std::fs::symlink_metadata(&path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(io_at(&path)(e)),
        };
        if !exists {
            return Ok(None);
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return Err(io_at(&path)(e)),
        };
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| Error::Json { path: path.clone() })?;
        let socket = v["socket"]
            .as_str()
            .filter(|s| s.starts_with('/'))
            .ok_or_else(|| Error::Invalid("key service socket must be absolute".into()))?;
        Ok(Some(socket.into()))
    }

    /// The key held under `var` (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`), from the one key store.
    pub fn key(&self, var: &str) -> Result<Secret> {
        if let Some(socket) = self.keysvc_socket()? {
            keysvc::require_present(&socket, var)?;
            return Ok(Secret::remote(socket, var));
        }
        keys::read(&self.keys_path(), var)
    }

    /// Whether the key store holds a usable key under `var` (the value is not handed out).
    pub fn has_key(&self, var: &str) -> bool {
        self.key(var).is_ok()
    }

    /// The route a label names in the layer's route table (provider, model, base, output cap, effort).
    pub fn route(&self, label: &str) -> Result<chat::Route> {
        chat::route(&self.routes_path(), label)
    }

    /// Reserve and admit one request for `ask.tool`, or refuse it unsent with the reason.
    pub fn admit(&self, ask: &Ask) -> std::result::Result<Admitted, Refusal> {
        money::admit(self, ask)
    }

    /// Settle one admitted request with how it ended.
    pub fn settle(&self, admitted: &Admitted, billing: Billing) -> Settled {
        money::settle(self, admitted, billing)
    }

    /// Settle an unresolved (or open) reservation of `tool` with an explicit charge and its evidence.
    pub fn reconcile(
        &self,
        tool: &str,
        id: &str,
        charge: u64,
        evidence: &str,
    ) -> Result<ledger::Totals> {
        money::reconcile(self, tool, id, charge, evidence)
    }

    /// One image request for `tool` (OpenAI's Image API form: a generation, or an edit with reference
    /// images): admitted under the tool's cap before it is sent, settled on the reply's usage. The
    /// reply carries the images (`data[].b64_json`); the receipt what was reserved and charged.
    pub fn image(
        &self,
        tool: &str,
        kind: images::Kind,
        body: &serde_json::Value,
        timeout_ms: u64,
    ) -> std::result::Result<(serde_json::Value, images::Receipt), images::Failure> {
        images::run(self, tool, kind, body, timeout_ms, &|| false)
    }

    /// Spend per tool and per day: settled, reserved, unresolved, cap and what is left.
    pub fn spend(&self) -> Result<ledger::Spend> {
        ledger::spend(self)
    }
}
