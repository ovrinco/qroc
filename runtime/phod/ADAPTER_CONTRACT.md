# T05 integer-range/1 executor and T04 admission handoff

Standalone Q-ROC PHOD leaf, additive to the frozen T01 PhodStore command seam.
No semantic source overlay. `source/semantic` is the exact T01 structural workspace,
kept separate from T04's admitted checker. `source/derive` selects retained
`types`, `codegen`, `codegen_rust` unchanged; its small library facade selects the
retained error/result definitions and CLI invokes `generate_rust`. It has no
corpus, network, provider or model dependency. Source identities are in the T05
receipt/evidence. Binaries are local offline builds, not fixtures.

The JSON profile accepts exactly `profile`, `name`, `purpose`, `min`, `max`,
`upper_policy`, `actor`, `rationale`, optional `trial_max`, `repair_budget`.
Profile is integer-range/1. Signed integer bounds must be between -1,000,000 and
1,000,000 with min < max. The upper policy is explicitly inclusive or exclusive;
there is no silent choice. Nonempty bounded actor/rationale record the user's
choice. Duplicate/unknown fields reject. Trial upper is an explicit alternative
candidate constant, not an alternative specification. Repair budget is 0 or 1;
it restores only that candidate constant to the selected bound. Seven or fewer
predeclared boundary/interior witnesses are finite claims, not exhaustive proof.

## Commands and state

- `phod-workbench-spec-save SPEC` writes exact normalized wire JSON to a new owned
  source archive and returns id/source_path/source_hash. It does not run tools.
- `phod-workbench {source_path,timeout_ms?}` starts a bounded explicit tool job.
  No candidate source code or admission path is accepted from arguments.
- `phod-job-cancel {id}` cancels only that job's tracked process group(s), retaining
  source and partial outputs. Terminal state is cancelled after owned processes stop.
- `phod-replay {id}` verifies recorded hashes and rechecks stored evidence through
  the same owner-admitted checker; it executes no candidate/compiler/derivation.
  A missing or changed checker refuses replay. Failed decisions remain failed.
- Effectful rerun is an explicit fresh `phod-<action> {source_path: old_archive}`.
  It creates a new job and never overwrites the old attempt.

The store creates ROOT/phod/admissions/JOB separately from outputs/JOB.
Manifest-pinned owner.py is a trusted runtime stage, run BEFORE candidate collection.
It validates human choices, freezes source bytes/ref, normalizes reviewed finite IR,
pins actual rustc version/binary digest, executor version/digest and environment,
and writes owner-admission.json plus plan/IR/owner-binding. The generated candidate
cannot supply those records. Generated code is a closed integer template with no
arbitrary source input; this is not an arbitrary-code security sandbox. Each
outside tool has closed stdin, bounded time/output, tracked process-group cleanup.

`owner-admission.json` exactly matches T04 Admission:
profile=qroc-phod-replay/1; source_bytes; source_ref; semantic_hash;
environment_digest; verifiers mapping obligation ID to name/version/digest.
`owner-binding.json` hashes source, IR, plan and admission and pins the selected
admitted checker. `plan.json` records fixed expected outputs before evidence.
Every candidate has compiler.json, witnesses.json, check-input.json,
structural-decision.json and decision.json. Workbench report retains all attempts,
repair counterexample, same semantic identity, artifact hashes and code-size/
implementation differences. Profile decision includes T04's evidence-set hash.
Actual compiler bytes and command records are independently observable artifacts.

## Required T08 integration

Merge the exact T04 semantic delta through its receipt, build the merged semantic
CLI, and bind its actual release binary. Set QROC_RESOURCE_ROOT to the installed
release root and select that binary with explicit QROC_PHOD_ADMITTED_BIN and
QROC_PHOD_ADMITTED_SHA256 (plain SHA-256). These are OWNER/runtime settings, not
fields in candidate/spec/evidence JSON. The owner verifies the selected digest
before collection and uses `check-admitted INPUT OWNER_ADMISSION`. Do not ship
T05's private evidence binary path, infer admission from tool labels, or substitute
structural `check`. Missing admission yields unavailable even when structural
checks pass. Release loading and integrated O3 acceptance remain unproved here.

Private leaf tests selected an exact unmodified copy of T04's receipt-bound binary,
SHA256 fcbc33bf5826aacd079200fe1ce7663297488134e34432d7c4ee6ed60f06d278.
That selection proves compatible binding, not the merged installed runtime.
`T04-admitted-handoff.json` records the receipt verification and private copy.
T08 must retain actual loaded binary/QML/root identities and repeat native,
unadmitted-verifier, missing-tool, counterexample/repair/replay and recovery checks.

Zig and the legacy parser remain unavailable; no fallback language passed.
The preserved legacy actions retain their own capabilities/errors and never route
through this profile. No model or GPU work, provider acquisition, global config,
installation, QREAM release, deployment or proposal activation occurs in this leaf.
