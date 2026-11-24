use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// A constraint value in the canonical spec.
/// Handles the mixed JSON types that appear in the constraints map:
/// strings, integers, booleans, and arrays of strings.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ConstraintValue {
    Boolean(bool),
    Integer(i64),
    String(String),
    StringList(Vec<String>),
}

impl fmt::Display for ConstraintValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstraintValue::Boolean(b) => write!(f, "{}", b),
            ConstraintValue::Integer(n) => write!(f, "{}", n),
            ConstraintValue::String(s) => write!(f, "{}", s),
            ConstraintValue::StringList(v) => {
                write!(f, "[")?;
                for (i, s) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\"", s)?;
                }
                write!(f, "]")
            }
        }
    }
}

/// A typed field declaration from the INTENT fields list.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TypedField {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
}

/// The intent section of a canonical spec.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct IntentSection {
    pub name: String,
    pub purpose: String,
    pub behavioral_contracts: Option<Vec<String>>,
    pub invariants: Option<Vec<String>>,
    pub fields: Option<Vec<TypedField>>,
}

/// The top-level canonical spec emitted by the Zig parser as JSON.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CanonicalSpec {
    pub schema_version: u32,
    pub source_path: String,
    pub source_hash: String,
    pub intent: IntentSection,
    pub constraints: BTreeMap<String, ConstraintValue>,
    pub derived_present: bool,
}
