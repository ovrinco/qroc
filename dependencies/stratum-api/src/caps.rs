//! One cap per tool (`caps.json`, schema stratum-caps/1): a total, and a per-day cap where set (the
//! calendar day in local time). A tool with no cap is refused. Read again on every request, so a cap
//! is changed by editing the file, without a restart.
//!
//! ```json
//! {"schema":"stratum-caps/1","currency":"USD",
//!  "tools":{"qroc":{"total":"100"},"zeno":{"total":"3","per_day":"1"}}}
//! ```
use crate::prices::amount;
use serde_json::Value;
use std::{fs, path::Path};

pub const SCHEMA: &str = "stratum-caps/1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cap {
    pub total: u64,
    pub per_day: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Caps {
    pub currency: String,
    pub tools: Vec<(String, Cap)>,
}

impl Caps {
    pub fn get(&self, tool: &str) -> Option<&Cap> {
        self.tools.iter().find(|(t, _)| t == tool).map(|(_, c)| c)
    }
}

/// The cap table, or why it cannot be used (the guard's refusal text).
pub fn load(path: &Path) -> Result<Caps, String> {
    let v: Value =
        serde_json::from_slice(&fs::read(path).map_err(|e| format!("cap file unreadable: {e}"))?)
            .map_err(|_| "cap file is not JSON".to_string())?;
    if v["schema"] != SCHEMA {
        return Err("cap file schema is not stratum-caps/1".into());
    }
    let currency = v["currency"]
        .as_str()
        .ok_or("cap file has no currency")?
        .to_owned();
    let obj = v["tools"].as_object().ok_or("cap file has no tools")?;
    let mut tools = Vec::new();
    for (tool, c) in obj {
        if !valid_tool(tool) {
            return Err(format!("cap file names an invalid tool '{tool}'"));
        }
        let total = amount(&c["total"]).ok_or(format!("cap {tool}.total missing or not exact"))?;
        let per_day = match c.get("per_day") {
            None | Some(Value::Null) => None,
            Some(x) => Some(amount(x).ok_or(format!("cap {tool}.per_day not exact"))?),
        };
        tools.push((tool.clone(), Cap { total, per_day }));
    }
    Ok(Caps { currency, tools })
}

/// A tool name: lower-case letters, digits and `-`, as it appears in the service path.
pub fn valid_tool(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 32
        && t.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_read_and_tool_names_checked() {
        let d = std::env::temp_dir().join(format!("stratum-caps-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let p = d.join("caps.json");
        fs::write(&p, r#"{"schema":"stratum-caps/1","currency":"USD","tools":{"zeno":{"total":"3","per_day":"1"},"qroc":{"total":100}}}"#).unwrap();
        let c = load(&p).unwrap();
        assert_eq!(
            c.get("zeno"),
            Some(&Cap {
                total: 3_000_000_000,
                per_day: Some(1_000_000_000)
            })
        );
        assert_eq!(c.get("qroc").unwrap().per_day, None);
        assert!(c.get("qream").is_none());
        fs::write(
            &p,
            r#"{"schema":"stratum-caps/1","currency":"USD","tools":{"Bad/Tool":{"total":"1"}}}"#,
        )
        .unwrap();
        assert!(load(&p).is_err());
        let _ = fs::remove_file(&p);
        let _ = fs::remove_dir(&d);
    }
}
