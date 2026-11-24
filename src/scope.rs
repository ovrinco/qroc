//! Revisioned, branch-owned scope decisions; selection never runs generation.
use crate::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
pub struct ScopeStore {
    root: PathBuf,
}
/// A triggered domain stays predicted until it has a durable document. Product
/// completion comes from the whole-pipeline receipt, rather than a navigation press.
pub fn domain_overview(documents: &BTreeSet<String>, product_complete: bool) -> Result<Vec<Value>> {
    let catalog = crate::registry::catalog_types();
    crate::registry::DOMAIN_ORDER.iter().map(|id| {
        let mut produced = Vec::new();
        let mut remaining = Vec::new();
        for kind in &catalog {
            if crate::registry::entry(kind)?["domain"] == *id {
                if documents.contains(kind) { produced.push(kind.clone()); }
                else { remaining.push(kind.clone()); }
            }
        }
        let complete = if *id == "PRODUCT" { product_complete } else { !produced.is_empty() };
        // The existing catalog supplies the portions behind a selected prediction;
        // no generated text or new requirements are inferred from selecting it.
        let projected = if complete { remaining } else { Vec::new() };
        // Documents of a domain that is not produced (a failed gate's drafts) are drafts,
        // never produced items: only a produced domain lists produced items.
        let (produced, drafted) = if complete { (produced, Vec::new()) } else { (Vec::new(), produced) };
        Ok(json!({"id":id,"name":id.to_ascii_lowercase(),"purpose":crate::registry::domain_purpose(id),
            "folder":crate::registry::domain_folder(id),
            "status":if complete {"produced"} else {"predicted"},"produced_items":produced,
            "drafted_items":drafted,"predicted_items":projected}))
    }).collect()
}
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
pub(crate) fn validate_root(root: &Path) -> Result<()> {
    if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid(
            "Scope/registry root must be absolute without traversal",
        ));
    }
    let mut p = PathBuf::new();
    for c in root.components() {
        p.push(c);
        if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid("Scope/registry root may not contain symlinks"));
        }
    }
    Ok(())
}
fn key(v: &Value, name: &str) -> Result<String> {
    let s = v[name].as_str().unwrap_or("");
    if s.is_empty()
        || s.len() > 200
        || !s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(invalid("Invalid scope project/branch identity"));
    }
    Ok(s.into())
}
impl ScopeStore {
    pub fn open(root: &Path) -> Result<Self> {
        validate_root(root)?;
        Ok(Self { root: root.into() })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    fn directory(&self, args: &Value) -> Result<PathBuf> {
        let p = key(args, "project_id")?;
        let b = key(args, "branch_id")?;
        Ok(self.root.join(p).join(b))
    }
    pub fn current(&self, args: &Value) -> Result<Option<Value>> {
        validate_root(&self.root)?;
        let dir = self.directory(args)?;
        validate_root(&dir)?;
        if !dir.exists() {
            return Ok(None);
        }
        let mut records = Vec::new();
        for e in fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let m = fs::symlink_metadata(&p)?;
            if !m.is_file() || m.len() > 1024 * 1024 {
                return Err(invalid("Invalid scope record"));
            }
            let v: Value = serde_json::from_slice(&fs::read(p)?)?;
            records.push(v);
        }
        records.sort_by_key(|v| v["revision"].as_u64().unwrap_or(0));
        Ok(records.pop())
    }
    pub fn command(&mut self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "qroc-scope-preview" => resolve(args),
            "qroc-scope-get" => Ok(json!({"scope":self.current(args)?})),
            "qroc-scope-save" => {
                let mut record = resolve(args)?;
                let dir = self.directory(args)?;
                // Verify actual durable owner identities, never create another project's state.
                let owner = self
                    .root
                    .parent()
                    .ok_or_else(|| invalid("Missing owner root"))?
                    .join("qroc");
                let p = key(args, "project_id")?;
                let b = key(args, "branch_id")?;
                let project: Value = serde_json::from_slice(&fs::read(
                    owner.join("projects").join(format!("{p}.json")),
                )?)?;
                let branch: Value = serde_json::from_slice(&fs::read(
                    owner.join("branches").join(format!("{b}.json")),
                )?)?;
                if project["id"] != p || branch["id"] != b || branch["project_id"] != p {
                    return Err(invalid("Scope owner mismatch"));
                }
                let current = self
                    .current(args)?
                    .map(|v| v["revision"].as_u64().unwrap_or(0))
                    .unwrap_or(0);
                if args["source_revision"].as_u64() != Some(current) {
                    return Err(invalid("Scope changed; exact source_revision required"));
                }
                record["revision"] = json!(current + 1);
                // Identity is obtained from the owner's records, never caller assertions.
                let mut refs = Vec::new();
                let docs = owner.join("documents").join(&p).join(&b);
                if docs.exists() {
                    for e in fs::read_dir(docs)? {
                        let path = e?.path();
                        if path.extension().and_then(|s| s.to_str()) == Some("json") {
                            let d: Value = serde_json::from_slice(&fs::read(path)?)?;
                            refs.push(json!({"id":d["id"],"document_type":d["document_type"],"revision":d["revision"],"sha256":d["sha256"]}));
                        }
                    }
                }
                record["document_refs"] = json!(refs);
                record["gate_reviews"] = json!(
                    project["gate_reviews"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|r| r["branch_id"] == b)
                        .collect::<Vec<_>>()
                );
                if serde_json::to_vec(&record)?.len() > 512 * 1024 {
                    return Err(invalid("Scope record exceeds 512 KiB"));
                }
                validate_root(&dir)?;
                fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{}.json", current + 1));
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(path)?;
                file.write_all(&serde_json::to_vec_pretty(&record)?)?;
                file.sync_all()?;
                Ok(record)
            }
            _ => Err(invalid(
                "capability-unavailable: scope adapter is not implemented",
            )),
        }
    }
}
pub fn resolve(args: &Value) -> Result<Value> {
    let project = key(args, "project_id")?;
    let branch = key(args, "branch_id")?;
    let nature = args["nature"].as_str().unwrap_or("");
    let defaults: &[&str] = match nature {
        "internal" => &[
            "STRATEGY_BRIEF",
            "PRD",
            "TECHNICAL_ARCHITECTURE",
            "VALIDATION_PLAN",
        ],
        "research" => &[
            "STRATEGY_BRIEF",
            "PRE_MORTEM",
            "VALIDATION_PLAN",
            "DESIGN_ALTERNATIVES",
        ],
        "creative" => &["BRAND_GUIDE", "MESSAGING_FRAMEWORK", "VISUAL_ASSETS"],
        "external-product" => &[
            "STRATEGY_BRIEF",
            "PRD",
            "VALIDATION_PLAN",
            "MARKET_ANALYSIS",
            "COMPLIANCE_PRIVACY",
        ],
        _ => {
            return Err(invalid(
                "Choose internal, research, creative or external-product scope",
            ));
        }
    };
    let rationale = args["rationale"]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 8192)
        .ok_or_else(|| invalid("Scope rationale is required"))?;
    let mut included = BTreeSet::new();
    if let Some(v) = args.get("included") {
        for k in v
            .as_array()
            .ok_or_else(|| invalid("included must be an array"))?
        {
            included.insert(crate::qroc::normalized_document(k.as_str().unwrap_or(""))?);
        }
    } else {
        included.extend(defaults.iter().map(|s| s.to_string()));
    }
    let overrides = args.get("excluded").cloned().unwrap_or(json!({}));
    let overrides = overrides
        .as_object()
        .ok_or_else(|| invalid("excluded must map types to reasons"))?;
    for (k, v) in overrides {
        if crate::qroc::normalized_document(k)? != *k {
            return Err(invalid("Exclusion keys must be canonical document IDs"));
        }
        if v.as_str().is_none_or(|s| s.trim().is_empty()) {
            return Err(invalid("Exclusions require reasons"));
        }
        included.remove(k);
    }
    let mut excluded = serde_json::Map::new();
    for k in crate::registry::catalog_types() {
        if !included.contains(&k) {
            excluded.insert(
                k.clone(),
                overrides.get(&k).cloned().unwrap_or(json!(format!(
                    "Not selected for {nature}; constraints remain applicable"
                ))),
            );
        }
    }
    let constraints = args.get("constraints").cloned().unwrap_or(json!([]));
    if constraints.as_array().is_none_or(|v| {
        v.len() > 128
            || v.iter()
                .any(|s| s.as_str().is_none_or(|s| s.is_empty() || s.len() > 8192))
    }) {
        return Err(invalid("constraints must be bounded text entries"));
    }
    let policy = args["input_policy"]
        .as_str()
        .unwrap_or("selected-and-verified");
    if policy != "selected-and-verified" {
        return Err(invalid("Unsupported input policy"));
    }
    Ok(
        json!({"schema":"qroc-scope/1","registry_version":crate::registry::VERSION,"project_id":project,"branch_id":branch,"nature":nature,"included":included,"excluded":excluded,"constraints":constraints,"rationale":rationale,"input_policy":policy,"document_refs":[],"gate_reviews":[],"execution_authority":false}),
    )
}
