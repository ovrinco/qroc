#![forbid(unsafe_code)]

use phod_ir::{
    normalize, sha256_bytes, Annotation, Candidate, Claim, DerivationKind, DerivationRef, Edition,
    EvidenceRequirement, Intent, IrError, Obligation, SourceRef, DIALECT,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug)]
pub enum LegacyError {
    Json(serde_json::Error),
    Missing(&'static str),
    Invalid(&'static str),
    UnsupportedVersion(u64),
    Ir(IrError),
}

impl fmt::Display for LegacyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid legacy JSON: {error}"),
            Self::Missing(field) => write!(f, "missing legacy field: {field}"),
            Self::Invalid(field) => write!(f, "invalid legacy field: {field}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported legacy schema version: {version}")
            }
            Self::Ir(error) => write!(f, "invalid lowered IR: {error}"),
        }
    }
}

impl std::error::Error for LegacyError {}

impl From<serde_json::Error> for LegacyError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<IrError> for LegacyError {
    fn from(value: IrError) -> Self {
        Self::Ir(value)
    }
}

/// Lowers Zig canonical JSON v0/v1 into normalized candidate IR.
///
/// # Errors
///
/// Returns [`LegacyError`] for malformed JSON, absent or invalid required fields, unsupported schema editions, or invalid normalized IR.
pub fn adapt_json(bytes: &[u8]) -> Result<Candidate, LegacyError> {
    let value: Value = serde_json::from_slice(bytes)?;
    adapt_value(&value)
}

/// Lowers an already parsed Zig canonical JSON value into normalized candidate IR.
///
/// # Errors
///
/// Returns [`LegacyError`] for absent or invalid required fields, unsupported schema editions, or invalid normalized IR.
#[allow(clippy::too_many_lines)] // The adapter is an explicit loss-policy ledger; keeping each legacy field handling visible prevents silent drops.
pub fn adapt_value(value: &Value) -> Result<Candidate, LegacyError> {
    let root = value.as_object().ok_or(LegacyError::Invalid("root"))?;
    let version = root
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or(LegacyError::Missing("schema_version"))?;
    let edition = match version {
        0 => Edition::PhodV0,
        1 => Edition::PhodV1,
        other => return Err(LegacyError::UnsupportedVersion(other)),
    };
    let source_hash = string(root.get("source_hash"), "source_hash")?;
    let raw_ref = string(root.get("source_path"), "source_path")?;
    let legacy_intent = root
        .get("intent")
        .and_then(Value::as_object)
        .ok_or(LegacyError::Missing("intent"))?;
    let name = string(legacy_intent.get("name"), "intent.name")?;
    let purpose = string(legacy_intent.get("purpose"), "intent.purpose")?;
    let target_language_value = root.get("constraints").and_then(Value::as_object).and_then(|constraints| constraints.get("target.language"));
    let target_language = match target_language_value {
        Some(value) => value.as_str().ok_or(LegacyError::Invalid("constraints.target.language"))?,
        None => "text", // preserved historical default; not admitted by replay/1
    };

    let mut entities = BTreeMap::new();
    entities.insert("target.language".into(), target_language.to_owned());
    if let Some(runtime) = root
        .get("constraints")
        .and_then(Value::as_object)
        .and_then(|constraints| constraints.get("target.runtime"))
    {
        entities.insert("target.runtime".into(), canonical_value(runtime)?);
    }
    if let Some(fields) = legacy_intent.get("fields") {
        let fields = fields.as_array().ok_or(LegacyError::Invalid("intent.fields"))?;
        let mut names = BTreeSet::new();
        for field in fields {
            let field_name = field
                .get("name")
                .and_then(Value::as_str)
                .ok_or(LegacyError::Invalid("intent.fields[].name"))?;
            if !names.insert(field_name) || entities.contains_key(field_name) {
                return Err(LegacyError::Invalid("duplicate intent field"));
            }
            let field_type = field
                .get("type")
                .ok_or(LegacyError::Invalid("intent.fields[].type"))?;
            entities.insert(field_name.to_owned(), canonical_value(field_type)?);
        }
    }

    let mut obligations = Vec::new();
    for key in ["behavioral_contracts", "hard_constraints", "invariants"] {
        if let Some(assertions) = legacy_intent.get(key) {
            let assertions = assertions
                .as_array()
                .ok_or(LegacyError::Invalid("intent assertion list"))?;
            for assertion in assertions {
                let statement = assertion
                    .as_str()
                    .ok_or(LegacyError::Invalid("intent assertion"))?;
                obligations.push(obligation(
                    &format!("legacy.intent.{key}.{}", stable_suffix(statement)),
                    &format!("intent.{key}"),
                    Claim::Assertion {
                        statement: statement.to_owned(),
                    },
                    None,
                ));
            }
        }
    }

    let constraints = root
        .get("constraints")
        .and_then(Value::as_object)
        .ok_or(LegacyError::Missing("constraints"))?;
    let mut seen_checks = BTreeSet::new();
    if let Some(checks) = constraints.get("verification.required_checks") {
        let checks = checks.as_array().ok_or(LegacyError::Invalid(
            "constraints.verification.required_checks",
        ))?;
        for check in checks {
            let check = check.as_str().ok_or(LegacyError::Invalid(
                "constraints.verification.required_checks[]",
            ))?;
            if seen_checks.insert(check) {
                let claim = match check {
                    "compile" => Claim::Compile {
                        language: target_language.to_owned(),
                    },
                    "syntax" => Claim::Syntax {
                        language: target_language.to_owned(),
                    },
                    "stdout_exact_match" => Claim::Unsupported {
                        constraint: "stdout_exact_match".into(),
                        reason: "legacy input has no exact expected stdout binding".into(),
                    },
                    "exit_code_zero" => Claim::RuntimeExact {
                        stdout: None,
                        exit_code: Some(0),
                    },
                    unknown => Claim::Unsupported {
                        constraint: format!("verification.required_checks:{unknown}"),
                        reason: "legacy checker name has no typed phod-ir/1 mapping".into(),
                    },
                };
                obligations.push(obligation(
                    &format!("legacy.check.{}", stable_suffix(check)),
                    &format!("verification.required_checks.{check}"),
                    claim,
                    None,
                ));
            }
        }
    } else {
        obligations.push(obligation(
            "legacy.check.default_compile",
            "verification.required_checks.compile",
            Claim::Compile {
                language: target_language.to_owned(),
            },
            None,
        ));
    }

    let mut annotations = Vec::new();
    for (key, raw) in constraints {
        let encoded = canonical_value(raw)?;
        annotations.push(Annotation {
            id: format!("legacy.constraint.{}", stable_suffix(key)),
            key: key.clone(),
            value: encoded.clone(),
        });
        if matches!(
            key.as_str(),
            "target.language" | "target.runtime" | "verification.required_checks"
        ) {
            continue;
        }
        let claim = if is_mapped_property(key) {
            Claim::Property {
                predicate: format!("{key}={encoded}"),
            }
        } else {
            Claim::Unsupported {
                constraint: format!("{key}={encoded}"),
                reason: "unknown required legacy constraint".into(),
            }
        };
        obligations.push(obligation(
            &format!("legacy.constraint.{}", stable_suffix(key)),
            key,
            claim,
            None,
        ));
    }
    for (key, raw) in legacy_intent {
        if matches!(
            key.as_str(),
            "name"
                | "purpose"
                | "fields"
                | "behavioral_contracts"
                | "hard_constraints"
                | "invariants"
        ) {
            continue;
        }
        annotations.push(Annotation {
            id: format!("legacy.intent.{}", stable_suffix(key)),
            key: format!("intent.{key}"),
            value: canonical_value(raw)?,
        });
        obligations.push(obligation(
            &format!("legacy.intent.{}", stable_suffix(key)),
            &format!("intent.{key}"),
            Claim::Unsupported {
                constraint: format!("intent.{key}={}", canonical_value(raw)?),
                reason: "legacy intent field has no typed phod-ir/1 mapping".into(),
            },
            None,
        ));
    }
    if let Some(derived) = root.get("derived_present") {
        annotations.push(Annotation {
            id: "legacy.derived_present".into(),
            key: "derived_present".into(),
            value: canonical_value(derived)?,
        });
    }
    if let Some(schema_type) = root.get("schema_type") {
        annotations.push(Annotation {
            id: "legacy.schema_type".into(),
            key: "schema_type".into(),
            value: canonical_value(schema_type)?,
        });
    }
    for (key, raw) in root {
        if matches!(
            key.as_str(),
            "constraints"
                | "derived_present"
                | "intent"
                | "schema_type"
                | "schema_version"
                | "source_hash"
                | "source_path"
        ) {
            continue;
        }
        let encoded = canonical_value(raw)?;
        annotations.push(Annotation {
            id: format!("legacy.top.{}", stable_suffix(key)),
            key: key.clone(),
            value: encoded.clone(),
        });
        obligations.push(obligation(
            &format!("legacy.top.{}", stable_suffix(key)),
            key,
            Claim::Unsupported {
                constraint: format!("{key}={encoded}"),
                reason: "unknown legacy top-level field".into(),
            },
            None,
        ));
    }

    let canonical_legacy = canonical_value(value)?;
    let parse_output = sha256_bytes(canonical_legacy.as_bytes());
    normalize(Candidate {
        dialect: DIALECT.into(),
        edition,
        semantic_hash: String::new(),
        source: SourceRef {
            hash: source_hash,
            raw_ref,
        },
        intent: Intent {
            name,
            purpose,
            entities,
        },
        obligations,
        annotations,
        derivation: vec![DerivationRef {
            step_id: "legacy.parse".into(),
            kind: DerivationKind::Parse,
            input_hashes: vec![],
            output_hash: parse_output,
        }],
        reservations: vec![],
    })
    .map_err(LegacyError::from)
}

fn is_mapped_property(key: &str) -> bool {
    // Namespace membership is not a supported semantic definition.
    let _ = key;
    false
}

fn obligation(id: &str, semantic_key: &str, claim: Claim, tool: Option<String>) -> Obligation {
    Obligation {
        id: id.to_owned(),
        semantic_key: semantic_key.to_owned(),
        required: true,
        evidence: EvidenceRequirement {
            kind: claim.evidence_kind(),
            tool,
        },
        claim,
    }
}

fn string(value: Option<&Value>, field: &'static str) -> Result<String, LegacyError> {
    value
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(LegacyError::Missing(field))
}

fn canonical_value(value: &Value) -> Result<String, LegacyError> {
    serde_json::to_string(value).map_err(LegacyError::from)
}

fn stable_suffix(value: &str) -> String {
    sha256_bytes(value.as_bytes())
        .trim_start_matches("sha256:")
        .chars()
        .take(16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_is_deterministic_across_json_key_order() {
        let first = br#"{"schema_version":1,"source_path":"x.phod","source_hash":"sha256:x","intent":{"name":"x","purpose":"p"},"constraints":{"target.language":"rust","verification.required_checks":["compile"]},"derived_present":false}"#;
        let second = br#"{"derived_present":false,"constraints":{"verification.required_checks":["compile"],"target.language":"rust"},"intent":{"purpose":"p","name":"x"},"source_hash":"sha256:x","source_path":"x.phod","schema_version":1}"#;
        assert_eq!(adapt_json(first).unwrap(), adapt_json(second).unwrap());
    }

    #[test]
    fn preserves_source_and_raw_reference() {
        let candidate = adapt_json(br#"{"schema_version":0,"source_path":"legacy.phod","source_hash":"sha256:raw","intent":{"name":"x","purpose":"p"},"constraints":{},"derived_present":false}"#).unwrap();
        assert_eq!(candidate.source.hash, "sha256:raw");
        assert_eq!(candidate.source.raw_ref, "legacy.phod");
        assert_eq!(candidate.edition, Edition::PhodV0);
    }

    #[test]
    fn unknown_required_check_and_constraint_lower_to_unsupported() {
        let candidate = adapt_json(br#"{"schema_version":1,"source_path":"x.phod","source_hash":"sha256:x","intent":{"name":"x","purpose":"p"},"constraints":{"verification.required_checks":["quantum_oracle"],"novel.required":true},"derived_present":false}"#).unwrap();
        let unsupported = candidate
            .obligations
            .iter()
            .filter(|obligation| obligation.claim.is_unsupported())
            .count();
        assert_eq!(unsupported, 2);
    }

    #[test]
    fn rejects_unknown_legacy_edition() {
        let error = adapt_json(br#"{"schema_version":2,"source_path":"x","source_hash":"h","intent":{"name":"x","purpose":"p"},"constraints":{}}"#).unwrap_err();
        assert!(matches!(error, LegacyError::UnsupportedVersion(2)));
    }
}
