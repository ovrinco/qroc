//! Experimental bounded stdlib contract, not the full PHOD textual grammar.
use crate::{normalize, sha256_bytes, Candidate, Claim, Edition, EvidenceRequirement, Intent, Obligation, SourceRef, DIALECT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const RECORD_PROFILE: &str = "qroc-record/1";
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSpec {
    pub profile: String,
    pub name: String,
    pub text_field: String,
    pub integer_field: String,
    pub minimum: i64,
    pub maximum: i64,
    pub unknown_fields: UnknownFields,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownFields { Reject, Preserve, Ignore }

impl RecordSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.profile != RECORD_PROFILE { return Err("unsupported_record_profile".into()); }
        if self.name.is_empty() || self.text_field.is_empty() || self.integer_field.is_empty() { return Err("empty_required_field".into()); }
        if self.text_field == self.integer_field { return Err("duplicate_field_identity".into()); }
        if self.minimum > self.maximum { return Err("invalid_integer_bounds".into()); }
        Ok(())
    }
    /// Error codes identify clauses; no implicit unknown-field policy or coercion.
    pub fn inspect(&self, record: &Value) -> Result<Value, String> {
        self.validate()?;
        let object = record.as_object().ok_or("record_not_object")?;
        if object.get(&self.text_field).and_then(Value::as_str).map_or(true, str::is_empty) { return Err(format!("nonempty_text:{}", self.text_field)); }
        let integer = object.get(&self.integer_field).and_then(Value::as_i64).ok_or_else(|| format!("integer_type:{}", self.integer_field))?;
        if integer < self.minimum || integer > self.maximum { return Err(format!("integer_range:{}", self.integer_field)); }
        let unknown = object.keys().any(|key| key != &self.text_field && key != &self.integer_field);
        if unknown && matches!(self.unknown_fields, UnknownFields::Reject) { return Err("unknown_field".into()); }
        if matches!(self.unknown_fields, UnknownFields::Ignore) {
            Ok(serde_json::json!({self.text_field.clone():object[&self.text_field],self.integer_field.clone():object[&self.integer_field]}))
        } else { Ok(record.clone()) }
    }
}

/// Binds raw UTF-8 JSON authority separately from normalized semantics.
pub fn lower(bytes: &[u8], raw_ref: &str) -> Result<Candidate, String> {
    let spec: RecordSpec = serde_json::from_slice(bytes).map_err(|e| format!("record_parse:{}:{}:{e}", e.line(), e.column()))?;
    spec.validate()?;
    let predicate = serde_json::to_string(&spec).map_err(|e| e.to_string())?;
    let claim = Claim::Property { predicate };
    normalize(Candidate {
        dialect: DIALECT.into(), edition: Edition::PhodV1, semantic_hash: String::new(),
        source: SourceRef { hash: sha256_bytes(bytes), raw_ref: raw_ref.into() },
        intent: Intent { name: spec.name, purpose: "Bounded record contract; no network or file effects in pure inspection".into(), entities: BTreeMap::new() },
        obligations: vec![Obligation { id:"record.contract".into(),semantic_key:RECORD_PROFILE.into(), required:true,evidence:EvidenceRequirement {kind:claim.evidence_kind(),tool:None},claim }],
        annotations:vec![],derivation:vec![],reservations:vec![],
    }).map_err(|e| e.to_string())
}

/// Flat object parser rejects duplicate top-level fields before map lowering.
/// Nested values in unknown fields are opaque, never interpreted as clauses.
pub fn parse_record(bytes: &[u8]) -> Result<Value, String> {
    struct Object;
    impl<'de> serde::de::Visitor<'de> for Object {
        type Value = Value;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("record object with unique fields") }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut result = serde_json::Map::new();
            while let Some((key, value)) = map.next_entry::<String, Value>()? {
                if result.contains_key(&key) { return Err(serde::de::Error::custom(format!("duplicate_record_field:{key}"))); }
                result.insert(key, value);
            }
            Ok(Value::Object(result))
        }
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = serde::de::Deserializer::deserialize_map(&mut deserializer, Object).map_err(|e| format!("record_parse:{}:{}:{e}", e.line(), e.column()))?;
    deserializer.end().map_err(|e| e.to_string())?;
    Ok(value)
}
