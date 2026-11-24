//! Native gate rubrics, read from a data file at run time.
//!
//! Each native gate (all but Gate 4, which keeps its semantic contract) has a meaning and
//! four to six named criteria whose maxima sum to 100. Thresholds and criterion floors stay
//! in the engine; the file cannot lower a bar. Replacing the file changes the gate prompts
//! with no code change.
use crate::{Error, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

pub const FILE_VARIABLE: &str = "QROC_GATE_RUBRICS";
const RELATIVE: [&str; 2] = ["runtime/gates/rubrics.json", "src/gates/rubrics.json"];

#[derive(Debug, Clone, Deserialize)]
pub struct Criterion {
    pub name: String,
    pub max: u32,
    pub question: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GateRubric {
    pub meaning: String,
    #[serde(default)]
    pub joins: Vec<String>,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Deserialize)]
struct RubricFile {
    schema: String,
    gates: BTreeMap<String, GateRubric>,
}

/// The engine's own bar for a gate: (threshold, per-criterion floor in percent).
pub fn bar(kind: &str) -> Option<(u32, u32)> {
    match kind {
        "GATE_2" => Some((92, 75)),
        "GATE_1" | "GATE_3" | "GATE_4" | "GATE_5" | "GATE_6" | "GATE_7" | "GATE_8" | "GATE_9"
        | "GATE_10" => Some((90, 70)),
        _ => None,
    }
}

/// `QROC_GATE_RUBRICS` names the file outright; otherwise the resource roots are searched.
pub fn path() -> Option<PathBuf> {
    if let Some(path) = env::var_os(FILE_VARIABLE) {
        return Some(PathBuf::from(path));
    }
    let mut roots = Vec::new();
    if let Some(root) = env::var_os("QROC_RESOURCE_ROOT") {
        roots.push(PathBuf::from(root));
    }
    if let Ok(exe) = env::current_exe()
        && let Some(root) = exe.parent().and_then(Path::parent)
    {
        roots.push(root.to_owned());
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    roots
        .iter()
        .flat_map(|root| RELATIVE.iter().map(move |rel| root.join(rel)))
        .find(|p| p.is_file())
}

pub fn load_from(path: &Path) -> Result<BTreeMap<String, GateRubric>> {
    let file: RubricFile = serde_json::from_slice(&fs::read(path)?)?;
    if file.schema != "qroc-gate-rubrics/1" {
        return Err(Error::Invalid("Unknown gate rubric schema".into()));
    }
    for (kind, rubric) in &file.gates {
        if kind == "GATE_4" || bar(kind).is_none() {
            return Err(Error::Invalid(format!(
                "{kind} is not a native gate with a rubric"
            )));
        }
        let total: u32 = rubric.criteria.iter().map(|c| c.max).sum();
        let mut names: Vec<String> = rubric
            .criteria
            .iter()
            .map(|c| c.name.trim().to_ascii_lowercase())
            .collect();
        names.sort();
        names.dedup();
        if !(4..=6).contains(&rubric.criteria.len())
            || total != 100
            || names.len() != rubric.criteria.len()
            || rubric.meaning.trim().is_empty()
            || rubric.criteria.iter().any(|c| {
                c.max == 0
                    || c.name.trim().is_empty()
                    || c.name.contains(':')
                    || c.name.contains('[')
            })
        {
            return Err(Error::Invalid(format!(
                "{kind} rubric needs 4 to 6 distinct named criteria with maxima summing to 100"
            )));
        }
    }
    Ok(file.gates)
}

pub fn rubric(kind: &str) -> Result<Option<GateRubric>> {
    let path = path().ok_or_else(|| Error::Invalid("Gate rubric file not found".into()))?;
    Ok(load_from(&path)?.remove(kind))
}

/// The gate's generation contract: what it decides, its criteria, its bar and the parser's form.
pub fn prompt(kind: &str, rubric: &GateRubric) -> Option<String> {
    let (threshold, floor) = bar(kind)?;
    let criteria = rubric
        .criteria
        .iter()
        .map(|c| format!("- {} (maximum {}): {}", c.name, c.max, c.question))
        .collect::<Vec<_>>()
        .join("\n");
    let form = rubric
        .criteria
        .iter()
        .map(|c| format!("- {}: <score>/{} - <feedback>", c.name, c.max))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "Produce a critical gate review that scores this gate.\n\
What this gate decides: {meaning}\n\
Documents it joins: {joins}\n\n\
Criteria (score each from 0 to its maximum; the maxima sum to 100):\n{criteria}\n\n\
Pass rule: the gate passes only if the total is at least {threshold} of 100 and every criterion scores at least {floor}% of its maximum. \
Score what the supplied documents show. A weak, vague or inconsistent plan must score low and fail; missing evidence lowers a criterion's score and is never a reason to withhold the score. \
This gate is decided by these scores; you are not granting approval.\n\n\
Required output form (read by a parser; follow it exactly):\n\
1. One line `[SCORE]: <total>` where <total> is the sum of the criterion scores. Write the marker exactly once and nowhere else.\n\
2. One line per criterion, with exactly these names and maxima, feedback after ` - `:\n{form}\n\
No other line beginning with '-' or '*' may contain both ':' and '/'; use numbered lists, tables or prose for everything else.\n\n\
After the score lines, write the sections: Decision; Evidence Reviewed; Blocking Gaps; Risks; Required Remediation; Next Review Conditions.",
        meaning = rubric.meaning,
        joins = if rubric.joins.is_empty() {
            "the supplied upstream documents".into()
        } else {
            rubric.joins.join(", ")
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_rubric_covers_every_native_gate_with_unchanged_bars() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/gates/rubrics.json");
        let gates = load_from(&path).expect("shipped rubric");
        let kinds: Vec<_> = gates.keys().cloned().collect();
        let mut want: Vec<String> = [1, 2, 3, 5, 6, 7, 8, 9, 10]
            .iter()
            .map(|n| format!("GATE_{n}"))
            .collect();
        want.sort();
        assert_eq!(kinds, want);
        assert_eq!(bar("GATE_1"), Some((90, 70)));
        assert_eq!(bar("GATE_2"), Some((92, 75)));
        let text = prompt("GATE_1", &gates["GATE_1"]).unwrap();
        assert!(text.contains("at least 90 of 100"));
        assert!(text.contains("[SCORE]: <total>"));
        assert!(text.contains("- Requirement Testability: <score>/25 - <feedback>"));
        assert!(!text.to_ascii_lowercase().contains("drafted"));
    }
}
