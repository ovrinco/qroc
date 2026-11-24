//! Pure, caller-admitted replay profile. Admission must come from the trusted owner,
//! never from a generated candidate or its evidence file.
use crate::{check, CheckInput, Decision, DecisionStatus, EvidenceAuthority, ToolRef};
use phod_ir::sha256_bytes;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROFILE: &str = "qroc-phod-replay/1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub profile: String,
    pub source_bytes: Vec<u8>,
    pub source_ref: String,
    pub semantic_hash: String,
    pub environment_digest: String,
    /// Exact obligation -> verifier identity, independently established by owner.
    pub verifiers: BTreeMap<String, ToolRef>,
}

#[derive(Debug, Serialize)]
pub struct ProfileDecision {
    pub profile: &'static str,
    pub evidence_hash: String,
    pub status: DecisionStatus,
    pub diagnostics: Vec<String>,
    pub replay: Decision,
}

/// No processes, IO, clock, or corpus capabilities. Caller owns admission trust.
#[must_use]
pub fn check_admitted(input: &CheckInput, admission: &Admission) -> ProfileDecision {
    let mut diagnostics = Vec::new();
    if admission.profile != PROFILE { diagnostics.push("unsupported_profile".into()); }
    if input.candidate.source.hash != sha256_bytes(&admission.source_bytes)
        || input.candidate.source.raw_ref != admission.source_ref {
        diagnostics.push("source_identity_mismatch".into());
    }
    if input.candidate.semantic_hash != admission.semantic_hash {
        diagnostics.push("unadmitted_semantics".into());
    }
    if input.environment.digest != admission.environment_digest {
        diagnostics.push("unadmitted_environment".into());
    }
    if input.candidate.obligations.is_empty() { diagnostics.push("empty_obligation_set".into()); }
    for obligation in &input.candidate.obligations {
        let supported = match &obligation.claim {
            phod_ir::Claim::Compile { language } | phod_ir::Claim::Syntax { language } => language == "rust",
            phod_ir::Claim::RuntimeExact { stdout, exit_code } => stdout.is_some() || exit_code.is_some(),
            phod_ir::Claim::Property { predicate } => serde_json::from_str::<phod_ir::record::RecordSpec>(predicate).is_ok_and(|s| s.validate().is_ok()),
            _ => false,
        };
        if !supported { diagnostics.push(format!("unsupported_profile_claim:{}", obligation.id)); }

        if obligation.required && !admission.verifiers.contains_key(&obligation.id) {
            diagnostics.push(format!("missing_verifier_admission:{}", obligation.id));
        }
        if let phod_ir::Claim::RuntimeExact { stdout: None, exit_code: None } = obligation.claim {
            diagnostics.push(format!("unbound_runtime_expectation:{}", obligation.id));
        }
    }
    for evidence in &input.evidence {
        if !matches!(evidence.authority, EvidenceAuthority::IndependentChecker | EvidenceAuthority::ExternalExecutor)
            || admission.verifiers.get(&evidence.obligation_id) != Some(&evidence.tool) {
            diagnostics.push(format!("untrusted_verifier:{}", evidence.id));
        }
        if evidence.tool.name.is_empty() || evidence.tool.version.is_empty() || evidence.tool.digest.is_empty() {
            diagnostics.push(format!("incomplete_verifier_identity:{}", evidence.id));
        }
    }
    // Sort entire serialized records: duplicate IDs remain hashed and rejected by replay.
    let mut records: Vec<_> = input.evidence.iter().map(|e| serde_json::to_string(e).expect("serializable evidence")).collect();
    records.sort();
    let evidence_hash = sha256_bytes(&serde_json::to_vec(&records).expect("serializable records"));
    let replay = check(input);
    diagnostics.sort();
    ProfileDecision {
        profile: PROFILE, evidence_hash,
        status: if diagnostics.is_empty() && replay.status == DecisionStatus::Accepted { DecisionStatus::Accepted } else { DecisionStatus::Rejected },
        diagnostics, replay,
    }
}
