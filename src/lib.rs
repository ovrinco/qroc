// Standalone domain library and explicit-root transport.
pub mod phod;
pub mod qroc;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

pub mod bridge;
pub mod creative;
pub mod decide;
pub mod gate_rubric;
pub mod harden;
pub mod intake;
pub mod media;
pub mod recon;
pub mod registry;
pub mod routes;
pub mod scope;

mod project_folders;

pub mod contracts;
pub mod pipeline;
pub mod worktable;
