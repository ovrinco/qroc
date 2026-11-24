pub mod types;
pub mod codegen;
pub mod codegen_rust;
use std::fmt;

/// Result of a successful derivation.
#[derive(Debug, Clone)]
pub struct DeriveResult {
    /// The generated artifact content (source code or text).
    pub artifact_content: String,
    /// The filename for the generated artifact.
    pub artifact_filename: String,
    /// The source hash from the spec, passed through for traceability.
    pub source_hash: String,
}

/// Errors that can occur during derivation.
#[derive(Debug)]
pub enum DeriveError {
    /// A required constraint is missing from the spec.
    MissingConstraint(String),
    /// A constraint has an unexpected type.
    InvalidConstraintType { key: String, expected: String },
    /// Internal formatting error during artifact generation.
    FormatError(fmt::Error),
    /// A field has an invalid or unknown type annotation.
    TypeError { field: String, expected: String },
}

impl fmt::Display for DeriveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeriveError::MissingConstraint(key) => {
                write!(f, "missing required constraint: {}", key)
            }
            DeriveError::InvalidConstraintType { key, expected } => {
                write!(
                    f,
                    "constraint '{}' has wrong type, expected {}",
                    key, expected
                )
            }
            DeriveError::FormatError(e) => {
                write!(f, "artifact formatting error: {}", e)
            }
            DeriveError::TypeError { field, expected } => {
                write!(f, "type error in field '{}': expected {}", field, expected)
            }
        }
    }
}

impl std::error::Error for DeriveError {}

impl From<fmt::Error> for DeriveError {
    fn from(e: fmt::Error) -> Self {
        DeriveError::FormatError(e)
    }
}

