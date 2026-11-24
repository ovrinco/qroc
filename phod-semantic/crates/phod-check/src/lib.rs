#![forbid(unsafe_code)]

use phod_ir::{
    semantic_hash, sha256_bytes, validate_shape, Candidate, Claim, EvidenceKind, DIALECT,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckInput {
    pub candidate: Candidate,
    pub artifact: ArtifactInput,
    pub environment: EnvironmentRef,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactInput {
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub manifest: BTreeMap<String, String>,
    pub declared: ArtifactRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub bytes_hash: String,
    pub manifest_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentRef {
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRef {
    pub name: String,
    pub version: String,
    pub digest: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAuthority {
    ExternalExecutor,
    IndependentChecker,
    DerivationAssertion,
    ManualAssertion,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOutcome {
    Proven,
    Refuted,
    Blocked,
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: String,
    pub obligation_id: String,
    pub semantic_hash: String,
    pub artifact: ArtifactRef,
    pub environment: EnvironmentRef,
    pub tool: ToolRef,
    pub authority: EvidenceAuthority,
    pub outcome: EvidenceOutcome,
    pub payload: EvidencePayload,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidencePayload {
    Compile,
    Syntax,
    RuntimeExact {
        stdout: String,
        exit_code: i32,
    },
    Property {
        seed: u64,
        cases: u64,
        counterexample: Option<Counterexample>,
    },
    Differential {
        reference_hash: String,
    },
    BoundedModel {
        bounds: ModelBounds,
        counterexample: Option<Counterexample>,
    },
    ManualBlocked {
        reason: String,
    },
    Unsupported {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

impl EvidencePayload {
    const fn kind(&self) -> EvidenceKind {
        match self {
            Self::Compile => EvidenceKind::Compile,
            Self::Syntax => EvidenceKind::Syntax,
            Self::RuntimeExact { .. } => EvidenceKind::RuntimeExact,
            Self::Property { .. } => EvidenceKind::Property,
            Self::Differential { .. } => EvidenceKind::Differential,
            Self::BoundedModel { .. } => EvidenceKind::BoundedModel,
            Self::ManualBlocked { .. } => EvidenceKind::ManualBlocked,
            Self::Unsupported { .. } => EvidenceKind::Unsupported,
            Self::Failed { .. } => EvidenceKind::Failed,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Counterexample {
    pub input: String,
    pub minimized: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBounds {
    pub depth: u64,
    pub states: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub status: DecisionStatus,
    pub semantic_hash: String,
    pub artifact: ArtifactRef,
    pub environment: EnvironmentRef,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: FindingCode,
    pub obligation_id: Option<String>,
    pub evidence_id: Option<String>,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCode {
    InvalidIr,
    UnsupportedDialect,
    SemanticHashMismatch,
    ArtifactHashMismatch,
    ManifestHashMismatch,
    ReservationConflict,
    UnsupportedObligation,
    MissingEvidence,
    DuplicateEvidence,
    UnknownObligation,
    CrossObligationEvidence,
    EvidenceHashMismatch,
    EnvironmentHashMismatch,
    ToolMismatch,
    AuthorityViolation,
    EvidenceRefuted,
    EvidenceBlocked,
    EvidenceUnsupported,
    EvidenceFailed,
    InvalidEvidence,
}

/// Evaluates a complete in-memory replay input without ambient capabilities.
#[must_use]
#[allow(clippy::too_many_lines)] // Keeping the fail-closed decision sequence linear makes the authority path easier to audit.
pub fn check(input: &CheckInput) -> Decision {
    let actual_artifact = ArtifactRef {
        bytes_hash: sha256_bytes(&input.artifact.bytes),
        manifest_hash: manifest_hash(&input.artifact.manifest),
    };
    let mut findings = Vec::new();

    if let Err(error) = validate_shape(&input.candidate) {
        push(
            &mut findings,
            FindingCode::InvalidIr,
            None,
            None,
            error.to_string(),
        );
    }
    if input.candidate.dialect != DIALECT {
        push(
            &mut findings,
            FindingCode::UnsupportedDialect,
            None,
            None,
            format!("expected {DIALECT}, got {}", input.candidate.dialect),
        );
    }
    match semantic_hash(&input.candidate) {
        Ok(actual) if actual != input.candidate.semantic_hash => push(
            &mut findings,
            FindingCode::SemanticHashMismatch,
            None,
            None,
            format!(
                "declared {}, computed {actual}",
                input.candidate.semantic_hash
            ),
        ),
        Err(error) => push(
            &mut findings,
            FindingCode::InvalidIr,
            None,
            None,
            error.to_string(),
        ),
        Ok(_) => {}
    }
    if input.artifact.declared.bytes_hash != actual_artifact.bytes_hash {
        push(
            &mut findings,
            FindingCode::ArtifactHashMismatch,
            None,
            None,
            "declared artifact bytes hash does not match supplied bytes".into(),
        );
    }
    if input.artifact.declared.manifest_hash != actual_artifact.manifest_hash {
        push(
            &mut findings,
            FindingCode::ManifestHashMismatch,
            None,
            None,
            "declared artifact manifest hash does not match supplied manifest".into(),
        );
    }

    let obligations: BTreeMap<_, _> = input
        .candidate
        .obligations
        .iter()
        .map(|obligation| (obligation.id.as_str(), obligation))
        .collect();
    for reservation in &input.candidate.reservations {
        if reservation.since <= input.candidate.edition {
            for obligation in input
                .candidate
                .obligations
                .iter()
                .filter(|obligation| obligation.semantic_key == reservation.semantic_key)
            {
                push(
                    &mut findings,
                    FindingCode::ReservationConflict,
                    Some(&obligation.id),
                    None,
                    format!(
                        "semantic key {} is reserved in {:?}",
                        reservation.semantic_key, reservation.since
                    ),
                );
            }
        }
    }
    for obligation in &input.candidate.obligations {
        if obligation.required && obligation.claim.is_unsupported() {
            push(
                &mut findings,
                FindingCode::UnsupportedObligation,
                Some(&obligation.id),
                None,
                "required obligation is unsupported".into(),
            );
        }
    }

    let mut evidence_ids = BTreeSet::new();
    let mut evidence_per_obligation: BTreeMap<&str, usize> = BTreeMap::new();
    for evidence in &input.evidence {
        if !evidence_ids.insert(evidence.id.as_str()) {
            push(
                &mut findings,
                FindingCode::DuplicateEvidence,
                Some(&evidence.obligation_id),
                Some(&evidence.id),
                "duplicate evidence id".into(),
            );
        }
        *evidence_per_obligation
            .entry(&evidence.obligation_id)
            .or_default() += 1;
        let Some(obligation) = obligations.get(evidence.obligation_id.as_str()) else {
            push(
                &mut findings,
                FindingCode::UnknownObligation,
                Some(&evidence.obligation_id),
                Some(&evidence.id),
                "evidence names no obligation in the candidate".into(),
            );
            continue;
        };
        validate_evidence(input, obligation, evidence, &actual_artifact, &mut findings);
    }
    for (obligation_id, count) in evidence_per_obligation {
        if count > 1 {
            push(
                &mut findings,
                FindingCode::DuplicateEvidence,
                Some(obligation_id),
                None,
                format!("{count} evidence records target one obligation"),
            );
        }
    }
    for obligation in input
        .candidate
        .obligations
        .iter()
        .filter(|obligation| obligation.required && !obligation.claim.is_unsupported())
    {
        if !input
            .evidence
            .iter()
            .any(|evidence| evidence.obligation_id == obligation.id)
        {
            push(
                &mut findings,
                FindingCode::MissingEvidence,
                Some(&obligation.id),
                None,
                "required obligation has no evidence".into(),
            );
        }
    }

    findings.sort_by(|a, b| {
        (
            a.obligation_id.as_deref(),
            a.evidence_id.as_deref(),
            a.code as u8,
            &a.detail,
        )
            .cmp(&(
                b.obligation_id.as_deref(),
                b.evidence_id.as_deref(),
                b.code as u8,
                &b.detail,
            ))
    });
    Decision {
        status: if findings.is_empty() {
            DecisionStatus::Accepted
        } else {
            DecisionStatus::Rejected
        },
        semantic_hash: input.candidate.semantic_hash.clone(),
        artifact: actual_artifact,
        environment: input.environment.clone(),
        findings,
    }
}

#[allow(clippy::too_many_lines)] // Every evidence rejection is intentionally visible in one exhaustive audit path.
fn validate_evidence(
    input: &CheckInput,
    obligation: &phod_ir::Obligation,
    evidence: &Evidence,
    actual_artifact: &ArtifactRef,
    findings: &mut Vec<Finding>,
) {
    let obligation_id = Some(obligation.id.as_str());
    let evidence_id = Some(evidence.id.as_str());
    if evidence.payload.kind() != obligation.evidence.kind {
        push(
            findings,
            FindingCode::CrossObligationEvidence,
            obligation_id,
            evidence_id,
            format!(
                "expected {:?}, got {:?}",
                obligation.evidence.kind,
                evidence.payload.kind()
            ),
        );
    }
    if evidence.semantic_hash != input.candidate.semantic_hash {
        push(
            findings,
            FindingCode::EvidenceHashMismatch,
            obligation_id,
            evidence_id,
            "evidence is bound to a different semantic hash".into(),
        );
    }
    if evidence.artifact != *actual_artifact {
        push(
            findings,
            FindingCode::EvidenceHashMismatch,
            obligation_id,
            evidence_id,
            "evidence is bound to different artifact bytes or manifest".into(),
        );
    }
    if evidence.environment != input.environment {
        push(
            findings,
            FindingCode::EnvironmentHashMismatch,
            obligation_id,
            evidence_id,
            "evidence is bound to a different environment".into(),
        );
    }
    if let Some(required_tool) = &obligation.evidence.tool {
        if evidence.tool.name != *required_tool {
            push(
                findings,
                FindingCode::ToolMismatch,
                obligation_id,
                evidence_id,
                format!("expected tool {required_tool}, got {}", evidence.tool.name),
            );
        }
    }
    if matches!(
        evidence.authority,
        EvidenceAuthority::DerivationAssertion | EvidenceAuthority::ManualAssertion
    ) && evidence.outcome == EvidenceOutcome::Proven
    {
        push(
            findings,
            FindingCode::AuthorityViolation,
            obligation_id,
            evidence_id,
            "a producer or manual assertion cannot prove its own claim".into(),
        );
    }
    match evidence.outcome {
        EvidenceOutcome::Proven => validate_proven_payload(
            &obligation.claim,
            &evidence.payload,
            obligation_id,
            evidence_id,
            findings,
        ),
        EvidenceOutcome::Refuted => {
            validate_counterexample(&evidence.payload, obligation_id, evidence_id, findings);
            push(
                findings,
                FindingCode::EvidenceRefuted,
                obligation_id,
                evidence_id,
                "evidence refutes the obligation".into(),
            );
        }
        EvidenceOutcome::Blocked => push(
            findings,
            FindingCode::EvidenceBlocked,
            obligation_id,
            evidence_id,
            "evidence is blocked".into(),
        ),
        EvidenceOutcome::Unsupported => push(
            findings,
            FindingCode::EvidenceUnsupported,
            obligation_id,
            evidence_id,
            "evidence reports unsupported semantics".into(),
        ),
        EvidenceOutcome::Failed => push(
            findings,
            FindingCode::EvidenceFailed,
            obligation_id,
            evidence_id,
            "evidence collection failed".into(),
        ),
    }
}

fn validate_proven_payload(
    claim: &Claim,
    payload: &EvidencePayload,
    obligation_id: Option<&str>,
    evidence_id: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    match (claim, payload) {
        (
            Claim::RuntimeExact { stdout, exit_code },
            EvidencePayload::RuntimeExact {
                stdout: actual_stdout,
                exit_code: actual_exit,
            },
        ) => {
            if stdout
                .as_ref()
                .is_some_and(|expected| expected != actual_stdout)
                || exit_code.is_some_and(|expected| expected != *actual_exit)
            {
                push(
                    findings,
                    FindingCode::InvalidEvidence,
                    obligation_id,
                    evidence_id,
                    "runtime output does not match the exact claim".into(),
                );
            }
        }
        (
            Claim::Differential { reference_hash },
            EvidencePayload::Differential {
                reference_hash: actual,
            },
        ) if reference_hash != actual => push(
            findings,
            FindingCode::InvalidEvidence,
            obligation_id,
            evidence_id,
            "differential evidence uses the wrong reference".into(),
        ),
        (
            _,
            EvidencePayload::Property {
                cases,
                counterexample,
                ..
            },
        ) => {
            if *cases == 0 || counterexample.is_some() {
                push(
                    findings,
                    FindingCode::InvalidEvidence,
                    obligation_id,
                    evidence_id,
                    "proven property evidence requires positive cases and no counterexample".into(),
                );
            }
        }
        (
            _,
            EvidencePayload::BoundedModel {
                bounds,
                counterexample,
            },
        ) => {
            if bounds.depth == 0 || bounds.states == 0 || counterexample.is_some() {
                push(
                    findings,
                    FindingCode::InvalidEvidence,
                    obligation_id,
                    evidence_id,
                    "proven bounded-model evidence requires positive bounds and no counterexample"
                        .into(),
                );
            }
        }
        _ => {}
    }
}

fn validate_counterexample(
    payload: &EvidencePayload,
    obligation_id: Option<&str>,
    evidence_id: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    let counterexample = match payload {
        EvidencePayload::Property {
            cases,
            counterexample,
            ..
        } => {
            if *cases == 0 {
                push(
                    findings,
                    FindingCode::InvalidEvidence,
                    obligation_id,
                    evidence_id,
                    "property evidence must record a positive case bound".into(),
                );
            }
            counterexample.as_ref()
        }
        EvidencePayload::BoundedModel {
            bounds,
            counterexample,
        } => {
            if bounds.depth == 0 || bounds.states == 0 {
                push(
                    findings,
                    FindingCode::InvalidEvidence,
                    obligation_id,
                    evidence_id,
                    "bounded-model evidence requires positive bounds".into(),
                );
            }
            counterexample.as_ref()
        }
        _ => None,
    };
    match counterexample {
        Some(counterexample)
            if !counterexample.input.is_empty() && !counterexample.minimized.is_empty() => {}
        _ => push(
            findings,
            FindingCode::InvalidEvidence,
            obligation_id,
            evidence_id,
            "refuted evidence requires an input and minimized counterexample".into(),
        ),
    }
}

fn manifest_hash(manifest: &BTreeMap<String, String>) -> String {
    let bytes =
        serde_json::to_vec(manifest).expect("BTreeMap<String, String> serialization is infallible");
    sha256_bytes(&bytes)
}

fn push(
    findings: &mut Vec<Finding>,
    code: FindingCode,
    obligation_id: Option<&str>,
    evidence_id: Option<&str>,
    detail: String,
) {
    findings.push(Finding {
        code,
        obligation_id: obligation_id.map(str::to_owned),
        evidence_id: evidence_id.map(str::to_owned),
        detail,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use phod_ir::{normalize, Edition, EvidenceRequirement, Intent, Obligation, SourceRef};

    fn base(claim: Claim, kind: EvidenceKind) -> CheckInput {
        let candidate = normalize(Candidate {
            dialect: DIALECT.into(),
            edition: Edition::PhodV1,
            semantic_hash: String::new(),
            source: SourceRef {
                hash: "sha256:source".into(),
                raw_ref: "fixture.phod".into(),
            },
            intent: Intent {
                name: "fixture".into(),
                purpose: "checker test".into(),
                entities: BTreeMap::new(),
            },
            obligations: vec![Obligation {
                id: "o1".into(),
                semantic_key: "check.o1".into(),
                required: true,
                claim,
                evidence: EvidenceRequirement { kind, tool: None },
            }],
            annotations: vec![],
            derivation: vec![],
            reservations: vec![],
        })
        .unwrap();
        let bytes = b"artifact".to_vec();
        let manifest = BTreeMap::new();
        let declared = ArtifactRef {
            bytes_hash: sha256_bytes(&bytes),
            manifest_hash: manifest_hash(&manifest),
        };
        CheckInput {
            candidate,
            artifact: ArtifactInput {
                bytes,
                manifest,
                declared,
            },
            environment: EnvironmentRef {
                digest: "sha256:env".into(),
            },
            evidence: vec![],
        }
    }

    fn evidence(input: &CheckInput, payload: EvidencePayload) -> Evidence {
        Evidence {
            id: "e1".into(),
            obligation_id: "o1".into(),
            semantic_hash: input.candidate.semantic_hash.clone(),
            artifact: input.artifact.declared.clone(),
            environment: input.environment.clone(),
            tool: ToolRef {
                name: "tester".into(),
                version: "1".into(),
                digest: "sha256:tool".into(),
            },
            authority: EvidenceAuthority::IndependentChecker,
            outcome: EvidenceOutcome::Proven,
            payload,
        }
    }

    #[test]
    fn valid_compile_evidence_accepts() {
        let mut input = base(
            Claim::Compile {
                language: "rust".into(),
            },
            EvidenceKind::Compile,
        );
        input
            .evidence
            .push(evidence(&input, EvidencePayload::Compile));
        assert_eq!(check(&input).status, DecisionStatus::Accepted);
    }

    #[test]
    fn unknown_check_regression_fails_closed() {
        let input = base(
            Claim::Unsupported {
                constraint: "mystery_check".into(),
                reason: "unknown".into(),
            },
            EvidenceKind::Unsupported,
        );
        let decision = check(&input);
        assert_eq!(decision.status, DecisionStatus::Rejected);
        assert!(decision
            .findings
            .iter()
            .any(|finding| finding.code == FindingCode::UnsupportedObligation));
    }

    #[test]
    fn missing_duplicate_cross_obligation_and_wrong_hash_evidence_reject() {
        let mut missing = base(
            Claim::Compile {
                language: "rust".into(),
            },
            EvidenceKind::Compile,
        );
        assert!(check(&missing)
            .findings
            .iter()
            .any(|finding| finding.code == FindingCode::MissingEvidence));
        let mut wrong = evidence(&missing, EvidencePayload::Syntax);
        wrong.semantic_hash = "sha256:wrong".into();
        let mut unknown = wrong.clone();
        unknown.id = "e2".into();
        unknown.obligation_id = "unknown-obligation".into();
        missing.evidence = vec![wrong.clone(), wrong, unknown];
        let decision = check(&missing);
        for code in [
            FindingCode::DuplicateEvidence,
            FindingCode::CrossObligationEvidence,
            FindingCode::EvidenceHashMismatch,
            FindingCode::UnknownObligation,
        ] {
            assert!(decision.findings.iter().any(|finding| finding.code == code));
        }
    }

    #[test]
    fn producer_assertion_has_no_acceptance_authority() {
        let mut input = base(
            Claim::Assertion {
                statement: "looks correct".into(),
            },
            EvidenceKind::Property,
        );
        let mut asserted = evidence(
            &input,
            EvidencePayload::Property {
                seed: 7,
                cases: 10,
                counterexample: None,
            },
        );
        asserted.authority = EvidenceAuthority::DerivationAssertion;
        input.evidence.push(asserted);
        assert!(check(&input)
            .findings
            .iter()
            .any(|finding| finding.code == FindingCode::AuthorityViolation));
    }

    #[test]
    fn reservation_applies_only_from_its_edition() {
        let mut old = base(
            Claim::Compile {
                language: "rust".into(),
            },
            EvidenceKind::Compile,
        );
        old.candidate.edition = phod_ir::Edition::PhodV0;
        old.candidate.reservations.push(phod_ir::Reservation {
            semantic_key: "check.o1".into(),
            since: phod_ir::Edition::PhodV1,
            reason: "retired".into(),
        });
        old.candidate = normalize(old.candidate).unwrap();
        old.evidence.push(evidence(&old, EvidencePayload::Compile));
        assert_eq!(check(&old).status, DecisionStatus::Accepted);
        let mut current = old.clone();
        current.candidate.edition = phod_ir::Edition::PhodV1;
        current.candidate = normalize(current.candidate).unwrap();
        current.evidence = vec![evidence(&current, EvidencePayload::Compile)];
        assert!(check(&current)
            .findings
            .iter()
            .any(|finding| finding.code == FindingCode::ReservationConflict));
    }

    #[test]
    fn property_seed_and_minimized_counterexample_are_replayable() {
        let mut input = base(
            Claim::Property {
                predicate: "x >= 0".into(),
            },
            EvidenceKind::Property,
        );
        let mut refuted = evidence(
            &input,
            EvidencePayload::Property {
                seed: 42,
                cases: 100,
                counterexample: Some(Counterexample {
                    input: "-12".into(),
                    minimized: "-1".into(),
                }),
            },
        );
        refuted.outcome = EvidenceOutcome::Refuted;
        input.evidence.push(refuted);
        let first = check(&input);
        let second = check(&input);
        assert_eq!(first, second);
        assert!(first
            .findings
            .iter()
            .any(|finding| finding.code == FindingCode::EvidenceRefuted));
    }

    #[test]
    fn bounded_model_evidence_requires_positive_bounds() {
        let mut input = base(
            Claim::BoundedModel {
                property: "no deadlock".into(),
            },
            EvidenceKind::BoundedModel,
        );
        input.evidence.push(evidence(
            &input,
            EvidencePayload::BoundedModel {
                bounds: ModelBounds {
                    depth: 0,
                    states: 20,
                },
                counterexample: None,
            },
        ));
        assert!(check(&input)
            .findings
            .iter()
            .any(|finding| finding.code == FindingCode::InvalidEvidence));
    }

    #[test]
    fn dialect_artifact_manifest_environment_and_tool_bindings_are_checked() {
        let mut input = base(
            Claim::Compile {
                language: "rust".into(),
            },
            EvidenceKind::Compile,
        );
        input.candidate.dialect = "phod-ir/99".into();
        input.candidate.obligations[0].evidence.tool = Some("rustc".into());
        input.candidate.semantic_hash = semantic_hash(&input.candidate).unwrap();
        input.artifact.declared.bytes_hash = "sha256:wrong-bytes".into();
        input.artifact.declared.manifest_hash = "sha256:wrong-manifest".into();
        let mut unbound = evidence(&input, EvidencePayload::Compile);
        unbound.environment.digest = "sha256:other-env".into();
        unbound.tool.name = "deriver".into();
        input.evidence.push(unbound);
        let decision = check(&input);
        for code in [
            FindingCode::UnsupportedDialect,
            FindingCode::ArtifactHashMismatch,
            FindingCode::ManifestHashMismatch,
            FindingCode::EvidenceHashMismatch,
            FindingCode::EnvironmentHashMismatch,
            FindingCode::ToolMismatch,
        ] {
            assert!(
                decision.findings.iter().any(|finding| finding.code == code),
                "missing {code:?}"
            );
        }
    }
}

/// Versioned admission layer; legacy check remains a structural replay API.
pub mod profile;
