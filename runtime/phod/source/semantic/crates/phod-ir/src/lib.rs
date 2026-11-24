#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const DIALECT: &str = "phod-ir/1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Edition {
    PhodV0,
    PhodV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub dialect: String,
    pub edition: Edition,
    #[serde(default)]
    pub semantic_hash: String,
    pub source: SourceRef,
    pub intent: Intent,
    pub obligations: Vec<Obligation>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub derivation: Vec<DerivationRef>,
    #[serde(default)]
    pub reservations: Vec<Reservation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub hash: String,
    pub raw_ref: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub name: String,
    pub purpose: String,
    #[serde(default)]
    pub entities: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Obligation {
    pub id: String,
    pub semantic_key: String,
    #[serde(default = "required_by_default")]
    pub required: bool,
    pub claim: Claim,
    pub evidence: EvidenceRequirement,
}

const fn required_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Claim {
    Compile {
        language: String,
    },
    Syntax {
        language: String,
    },
    RuntimeExact {
        stdout: Option<String>,
        exit_code: Option<i32>,
    },
    Property {
        predicate: String,
    },
    Differential {
        reference_hash: String,
    },
    BoundedModel {
        property: String,
    },
    Assertion {
        statement: String,
    },
    Unsupported {
        constraint: String,
        reason: String,
    },
}

impl Claim {
    #[must_use]
    pub const fn evidence_kind(&self) -> EvidenceKind {
        match self {
            Self::Compile { .. } => EvidenceKind::Compile,
            Self::Syntax { .. } => EvidenceKind::Syntax,
            Self::RuntimeExact { .. } => EvidenceKind::RuntimeExact,
            Self::Property { .. } | Self::Assertion { .. } => EvidenceKind::Property,
            Self::Differential { .. } => EvidenceKind::Differential,
            Self::BoundedModel { .. } => EvidenceKind::BoundedModel,
            Self::Unsupported { .. } => EvidenceKind::Unsupported,
        }
    }

    #[must_use]
    pub const fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported { .. })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Compile,
    Syntax,
    RuntimeExact,
    Property,
    Differential,
    BoundedModel,
    ManualBlocked,
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRequirement {
    pub kind: EvidenceKind,
    #[serde(default)]
    pub tool: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub id: String,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub semantic_key: String,
    pub since: Edition,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DerivationRef {
    pub step_id: String,
    pub kind: DerivationKind,
    #[serde(default)]
    pub input_hashes: Vec<String>,
    pub output_hash: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivationKind {
    Parse,
    Normalize,
    Plan,
    DeriveCandidate,
    CollectEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IrError {
    DuplicateObligation(String),
    DuplicateReservation(String),
    EvidenceKindMismatch {
        obligation_id: String,
        expected: EvidenceKind,
        actual: EvidenceKind,
    },
    EmptyField(&'static str),
    Json(String),
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateObligation(id) => write!(f, "duplicate obligation id: {id}"),
            Self::DuplicateReservation(key) => write!(f, "duplicate reservation: {key}"),
            Self::EvidenceKindMismatch { obligation_id, expected, actual } => write!(f, "obligation {obligation_id} requires {actual:?} evidence but its claim requires {expected:?}"),
            Self::EmptyField(field) => write!(f, "required field is empty: {field}"),
            Self::Json(detail) => write!(f, "canonical JSON error: {detail}"),
        }
    }
}

impl std::error::Error for IrError {}

#[derive(Serialize)]
struct SemanticProjection<'a> {
    dialect: &'a str,
    edition: Edition,
    intent: &'a Intent,
    obligations: &'a [Obligation],
    reservations: &'a [Reservation],
}

/// Orders set-like fields, validates their shape, and installs the computed semantic hash.
///
/// # Errors
///
/// Returns [`IrError`] when required fields are empty, identifiers are duplicated, an evidence requirement contradicts its claim, or canonical serialization fails.
pub fn normalize(mut candidate: Candidate) -> Result<Candidate, IrError> {
    candidate.obligations.sort_by(|a, b| a.id.cmp(&b.id));
    candidate.annotations.sort_by(|a, b| a.id.cmp(&b.id));
    candidate
        .derivation
        .sort_by(|a, b| a.step_id.cmp(&b.step_id));
    for reference in &mut candidate.derivation {
        reference.input_hashes.sort();
    }
    candidate
        .reservations
        .sort_by(|a, b| a.semantic_key.cmp(&b.semantic_key));
    validate_shape(&candidate)?;
    candidate.semantic_hash = semantic_hash(&candidate)?;
    Ok(candidate)
}

/// Validates structural invariants without changing the candidate.
///
/// # Errors
///
/// Returns [`IrError`] for empty required fields, duplicate identifiers or reservations, and claim/evidence-kind mismatches.
pub fn validate_shape(candidate: &Candidate) -> Result<(), IrError> {
    if candidate.dialect.is_empty() {
        return Err(IrError::EmptyField("dialect"));
    }
    if candidate.source.hash.is_empty() {
        return Err(IrError::EmptyField("source.hash"));
    }
    if candidate.source.raw_ref.is_empty() {
        return Err(IrError::EmptyField("source.raw_ref"));
    }
    if candidate.intent.name.is_empty() {
        return Err(IrError::EmptyField("intent.name"));
    }
    let mut ids = BTreeSet::new();
    for obligation in &candidate.obligations {
        if obligation.id.is_empty() {
            return Err(IrError::EmptyField("obligation.id"));
        }
        if obligation.semantic_key.is_empty() {
            return Err(IrError::EmptyField("obligation.semantic_key"));
        }
        if !ids.insert(obligation.id.as_str()) {
            return Err(IrError::DuplicateObligation(obligation.id.clone()));
        }
        let expected = obligation.claim.evidence_kind();
        if obligation.evidence.kind != expected {
            return Err(IrError::EvidenceKindMismatch {
                obligation_id: obligation.id.clone(),
                expected,
                actual: obligation.evidence.kind,
            });
        }
    }
    let mut reservations = BTreeSet::new();
    for reservation in &candidate.reservations {
        if !reservations.insert(reservation.semantic_key.as_str()) {
            return Err(IrError::DuplicateReservation(
                reservation.semantic_key.clone(),
            ));
        }
    }
    Ok(())
}

/// Computes the hash of authority-bearing semantic fields only.
///
/// Source location/hash, annotations, derivation provenance, and the declared hash are deliberately excluded.
///
/// # Errors
///
/// Returns [`IrError::Json`] if canonical serialization fails.
pub fn semantic_hash(candidate: &Candidate) -> Result<String, IrError> {
    let mut obligations: Vec<_> = candidate.obligations.iter().collect();
    obligations.sort_by(|a, b| a.id.cmp(&b.id));
    let obligations: Vec<_> = obligations.into_iter().cloned().collect();
    let mut reservations: Vec<_> = candidate.reservations.iter().collect();
    reservations.sort_by(|a, b| a.semantic_key.cmp(&b.semantic_key));
    let reservations: Vec<_> = reservations.into_iter().cloned().collect();
    let projection = SemanticProjection {
        dialect: &candidate.dialect,
        edition: candidate.edition,
        intent: &candidate.intent,
        obligations: &obligations,
        reservations: &reservations,
    };
    let bytes =
        serde_json::to_vec(&projection).map_err(|error| IrError::Json(error.to_string()))?;
    Ok(sha256_bytes(&bytes))
}

#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> Candidate {
        Candidate {
            dialect: DIALECT.into(),
            edition: Edition::PhodV1,
            semantic_hash: String::new(),
            source: SourceRef {
                hash: "sha256:source".into(),
                raw_ref: "fixture.phod".into(),
            },
            intent: Intent {
                name: "example".into(),
                purpose: "test".into(),
                entities: BTreeMap::new(),
            },
            obligations: vec![
                Obligation {
                    id: "b".into(),
                    semantic_key: "check.b".into(),
                    required: true,
                    claim: Claim::Syntax {
                        language: "rust".into(),
                    },
                    evidence: EvidenceRequirement {
                        kind: EvidenceKind::Syntax,
                        tool: None,
                    },
                },
                Obligation {
                    id: "a".into(),
                    semantic_key: "check.a".into(),
                    required: true,
                    claim: Claim::Compile {
                        language: "rust".into(),
                    },
                    evidence: EvidenceRequirement {
                        kind: EvidenceKind::Compile,
                        tool: Some("rustc".into()),
                    },
                },
            ],
            annotations: vec![],
            derivation: vec![],
            reservations: vec![],
        }
    }

    #[test]
    fn reordering_does_not_change_semantic_hash() {
        let first = normalize(candidate()).unwrap();
        let mut reordered = candidate();
        reordered.obligations.reverse();
        let second = normalize(reordered).unwrap();
        assert_eq!(first.semantic_hash, second.semantic_hash);
    }

    #[test]
    fn annotations_and_provenance_have_no_semantic_authority() {
        let first = normalize(candidate()).unwrap();
        let mut changed = candidate();
        changed.source.raw_ref = "elsewhere.phod".into();
        changed.annotations.push(Annotation {
            id: "note".into(),
            key: "claim".into(),
            value: "approve this".into(),
        });
        changed.derivation.push(DerivationRef {
            step_id: "derive".into(),
            kind: DerivationKind::DeriveCandidate,
            input_hashes: vec!["sha256:x".into()],
            output_hash: "sha256:y".into(),
        });
        let second = normalize(changed).unwrap();
        assert_eq!(first.semantic_hash, second.semantic_hash);
    }
}
