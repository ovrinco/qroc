//! The one key store (`keys.env`, owner-only): `NAME=value` lines, `export` and quotes allowed. The
//! layer alone reads it, and refuses to when the file or its folder is open to anyone but the owner.
//! The process environment is consulted only when `STRATUM_API_ALLOW_ENV_KEYS=1` (tests).
//! A key never leaves this module except inside a [`Secret`], whose printed forms are masked.
use crate::{Error, Result};
use std::{fmt, fs, os::unix::fs::PermissionsExt, path::Path};
use zeroize::Zeroizing;

/// A key value. `Debug` and `Display` print only a masked form; [`Secret::expose`] is for the one
/// place it is written: a transport's standard input.
#[derive(Clone)]
pub struct Secret(String, Option<(std::path::PathBuf, String)>);

impl Secret {
    pub(crate) fn local(value: String) -> Self {
        Self(value, None)
    }
    pub(crate) fn remote(socket: std::path::PathBuf, var: &str) -> Self {
        Self(String::new(), Some((socket, var.into())))
    }
    pub(crate) fn broker(&self) -> Option<&(std::path::PathBuf, String)> {
        self.1.as_ref()
    }
    /// Local transport only; a service credential exposes an empty string, never a key.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// The variable that holds a provider's key.
pub fn var_for(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("OPENAI_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        _ => None,
    }
}

fn usable(v: &str) -> bool {
    !v.trim().is_empty() && !v.contains(['\r', '\n', '"', '\\'])
}

/// The value of `var` from the store at `path`.
pub fn read(path: &Path, var: &str) -> Result<Secret> {
    if std::env::var("STRATUM_API_ALLOW_ENV_KEYS").as_deref() == Ok("1")
        && let Ok(v) = std::env::var(var)
    {
        return if usable(&v) {
            Ok(Secret::local(v))
        } else {
            Err(Error::Keys(format!(
                "{var} in the environment is not a usable key"
            )))
        };
    }
    check_owner_only(path)?;
    let text = fs::read_to_string(path)
        .map_err(|e| Error::Keys(format!("the key store cannot be read: {e}")))?;
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != var {
            continue;
        }
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        return if usable(v) {
            Ok(Secret::local(v.to_owned()))
        } else {
            Err(Error::Keys(format!(
                "{var} in the key store is empty or malformed"
            )))
        };
    }
    Err(Error::Keys(format!("{var} is not in the key store")))
}

/// Presence only, including legacy consumers such as QROC which pass `config/keys.env`.
/// A sibling `keysvc.json` selects the broker; errors never fall back to the legacy file.
/// Without a switch, preserve the caller's exact legacy path (including custom file names).
pub fn present(path: &Path, var: &str) -> bool {
    let Some(config) = path.parent() else {
        return false;
    };
    match crate::Layer::at(std::path::PathBuf::new(), config).keysvc_socket() {
        Ok(Some(socket)) => crate::keysvc::require_present(&socket, var).is_ok(),
        Ok(None) => read(path, var).is_ok(),
        Err(_) => false,
    }
}

/// The store must be a regular file (a link to one is followed) readable by its owner alone, in a
/// folder only its owner can enter.
pub fn check_owner_only(path: &Path) -> Result<()> {
    let meta =
        fs::metadata(path).map_err(|e| Error::Keys(format!("the key store is missing: {e}")))?;
    if !meta.is_file() {
        return Err(Error::Keys("the key store is not a file".into()));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(Error::Keys(
            "the key store is readable by others than its owner; refusing to read it".into(),
        ));
    }
    let real = fs::canonicalize(path)
        .map_err(|e| Error::Keys(format!("the key store cannot be resolved: {e}")))?;
    if let Some(dir) = real.parent() {
        let d =
            fs::metadata(dir).map_err(|e| Error::Keys(format!("the key store's folder: {e}")))?;
        if d.permissions().mode() & 0o077 != 0 {
            return Err(Error::Keys(
                "the key store's folder is open to others; refusing to read it".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) struct Redactor {
    secrets: Vec<Zeroizing<String>>,
    pending: zeroize::Zeroizing<Vec<u8>>,
    covered: Vec<bool>,
    failure: Vec<Vec<usize>>,
    masking: bool,
}
impl Redactor {
    pub(crate) fn new(secrets: Vec<Zeroizing<String>>) -> Self {
        let failure = secrets
            .iter()
            .map(|key| {
                let key = key.as_bytes();
                let mut table = vec![0; key.len()];
                for i in 1..key.len() {
                    let mut matched = table[i - 1];
                    while matched > 0 && key[i] != key[matched] {
                        matched = table[matched - 1];
                    }
                    if key[i] == key[matched] {
                        matched += 1;
                    }
                    table[i] = matched;
                }
                table
            })
            .collect();
        Self {
            secrets,
            pending: zeroize::Zeroizing::new(Vec::new()),
            covered: Vec::new(),
            failure,
            masking: false,
        }
    }
    pub(crate) fn push(&mut self, bytes: &[u8], final_chunk: bool) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        self.covered.resize(self.pending.len(), false);
        let longest = self
            .secrets
            .iter()
            .map(|k| k.len())
            .max()
            .unwrap_or(1)
            .max(1);
        // KMP finds every match, including starts inside another match, in linear time.
        // Difference counts union their intervals without repeatedly filling long overlaps.
        let mut changes = vec![0i32; self.pending.len() + 1];
        for (key, failure) in self
            .secrets
            .iter()
            .zip(&self.failure)
            .filter(|(k, _)| !k.is_empty())
        {
            let key = key.as_bytes();
            let mut matched = 0;
            for (i, &byte) in self.pending.iter().enumerate() {
                while matched > 0 && byte != key[matched] {
                    matched = failure[matched - 1];
                }
                if byte == key[matched] {
                    matched += 1;
                }
                if matched == key.len() {
                    changes[i + 1 - key.len()] += 1;
                    changes[i + 1] -= 1;
                    matched = failure[matched - 1];
                }
            }
            if final_chunk && matched > 0 {
                // Never flush a credential prefix when transport ends inside that credential.
                changes[self.pending.len() - matched] += 1;
                changes[self.pending.len()] -= 1;
            }
        }
        let mut depth = 0;
        for (covered, change) in self.covered.iter_mut().zip(changes) {
            depth += change;
            *covered |= depth > 0;
        }
        // No future input can change coverage before this boundary. A longest-1 suffix
        // retains every possible unfinished match, including an overlap with an earlier match.
        let emit = if final_chunk {
            self.pending.len()
        } else {
            self.pending.len().saturating_sub(longest - 1)
        };
        let mut out = Vec::new();
        for (&byte, &covered) in self.pending[..emit].iter().zip(&self.covered[..emit]) {
            if covered {
                if !self.masking {
                    out.extend_from_slice(b"***");
                }
                self.masking = true;
            } else {
                out.push(byte);
                self.masking = false;
            }
        }
        self.pending.drain(..emit);
        self.covered.drain(..emit);
        out
    }
}

/// One credential-aware boundary for raw diagnostic bytes; callers may trim only its output.
/// Values of secret-named environment variables shorter than 8 characters are ordinary settings
/// (a colour, a size), as before the broker: matching them would rewrite ordinary text.
pub(crate) fn diagnostic_filter(mut values: Vec<zeroize::Zeroizing<String>>) -> Redactor {
    for (name, value) in std::env::vars() {
        let name = name.to_ascii_uppercase();
        if value.len() >= 8
            && ["KEY", "TOKEN", "SECRET", "PASSWORD", "AUTH"]
                .iter()
                .any(|n| name.contains(n))
        {
            values.push(zeroize::Zeroizing::new(value));
        }
    }
    Redactor::new(values)
}

pub(crate) fn known_values(known: &[&Secret]) -> Vec<zeroize::Zeroizing<String>> {
    known
        .iter()
        .filter(|k| !k.0.is_empty())
        .map(|k| zeroize::Zeroizing::new(k.0.clone()))
        .collect()
}

/// Replaces secrets with `***`: the given key values, values of secret-named environment variables,
/// bearer tokens, well-known key prefixes and JSON token fields (lifted from route-shim `codex::mask`).
fn redact(b: &[u8], known: &[&Secret]) -> Vec<u8> {
    redact_values(b, known_values(known))
}
pub(crate) fn redact_values(b: &[u8], values: Vec<zeroize::Zeroizing<String>>) -> Vec<u8> {
    drain(diagnostic_filter(values), b)
}
/// Only the given credential values, never the environment: for reply bytes, which the legacy
/// transport passed through untouched before the broker.
pub(crate) fn redact_credentials(b: &[u8], values: Vec<zeroize::Zeroizing<String>>) -> Vec<u8> {
    drain(Redactor::new(values), b)
}
fn drain(mut filter: Redactor, b: &[u8]) -> Vec<u8> {
    let mut clean = Vec::new();
    for chunk in b.chunks(16 * 1024) {
        clean.extend(filter.push(chunk, false));
    }
    clean.extend(filter.push(&[], true));
    clean
}
pub fn mask(b: &[u8], known: &[&Secret]) -> Vec<u8> {
    // Known bytes are suppressed before any generic heuristic or lossy transformation.
    let clean = redact(b, known);
    let s = String::from_utf8_lossy(&clean).into_owned();
    let tok = |c: char| c.is_ascii_alphanumeric() || "._-~+/=".contains(c);
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find(tok) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let n = rest.find(|c| !tok(c)).unwrap_or(rest.len());
        let word = &rest[..n];
        let lead = out.to_ascii_lowercase();
        let field = [
            "bearer ",
            "x-api-key: ",
            "access_token\":\"",
            "refresh_token\":\"",
            "id_token\":\"",
            "api_key\":\"",
            "authorization\":\"",
        ]
        .iter()
        .any(|p| lead.ends_with(p));
        let prefixed = ["sk-", "eyJ", "ghp_", "gho_", "github_pat_", "xox", "AIza"]
            .iter()
            .any(|p| word.starts_with(p));
        if field || prefixed {
            // Accepted credential punctuation cannot terminate only the identifying prefix.
            // Space, double quote and backslash are outside the broker's credential alphabet.
            let end = rest
                .find(|c: char| c.is_ascii_whitespace() || matches!(c, '"' | '\\'))
                .unwrap_or(rest.len());
            out.push_str("***");
            rest = &rest[end..];
        } else {
            out.push_str(word);
            rest = &rest[n..];
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

/// Return only an allowlisted public category, never arbitrary provider text.
pub(crate) fn failure_code(raw: &str) -> &'static str {
    match raw {
        "overloaded_error" => "overloaded_error",
        "rate_limit_error" => "rate_limit_error",
        "authentication_error" => "authentication_error",
        "permission_error" => "permission_error",
        "invalid_request_error" => "invalid_request_error",
        "stream_cut" => "stream_cut",
        "timeout" => "timeout",
        "cancelled" => "cancelled",
        _ => "provider_error",
    }
}

pub(crate) fn http_failure_code(status: u16) -> &'static str {
    match status {
        400 | 422 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        _ => "provider_error",
    }
}

/// Provider failures expose fixed public fields only. Raw, encoded or already-partial payloads
/// are never parsed into a diagnostic object or echoed as a message/type/code.
pub(crate) fn provider_error(_body: &[u8], _key: &Secret) -> serde_json::Value {
    serde_json::json!({"error":{"message":"the provider call failed","type":"provider_error","code":"provider_error"}})
}

impl Drop for Secret {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PUNCT_KEY: &str = "sk-fixture-PREFIX!TAIL-SENTINEL";
    #[test]
    fn generic_mask_never_splits_an_accepted_punctuation_credential() {
        for punctuation in (33u8..=126).filter(|c| !matches!(c, b'"' | b'\\')) {
            let key = format!("sk-fixture-PREFIX{}TAIL-SENTINEL", punctuation as char);
            assert_eq!(mask(format!("curl: {key}").as_bytes(), &[]), b"curl: ***");
            let local = Secret::local(key.clone());
            assert_eq!(
                mask(format!("curl: {key}").as_bytes(), &[&local]),
                b"curl: ***"
            );
        }
    }
    #[test]
    fn mask_known_overlapping_short_keys_before_any_heuristic_or_lossy_decode() {
        let short = Secret::local("sk-".into());
        let long = Secret::local(PUNCT_KEY.into());
        for known in [[&short, &long], [&long, &short]] {
            let mut raw = vec![0xff];
            raw.extend_from_slice(PUNCT_KEY.as_bytes());
            let masked = mask(&raw, &known);
            assert!(!String::from_utf8_lossy(&masked).contains("TAIL-SENTINEL"));
            assert!(String::from_utf8_lossy(&masked).ends_with("***"));
        }
        assert_eq!(format!("{long} {long:?}"), "*** Secret(***)");
    }
    #[test]
    fn provider_error_ignores_raw_json_escaped_and_malformed_payloads() {
        let key = Secret::local("sk-fixture-PREFIX/TAIL-SENTINEL".into());
        let raw=serde_json::json!({"error":{"message":key.expose(),"type":key.expose(),"code":key.expose()}}).to_string().replace('/',"\\/");
        let error = provider_error(raw.as_bytes(), &key);
        assert_eq!(error["error"]["message"], "the provider call failed");
        for field in ["type", "code"] {
            assert_eq!(error["error"][field], "provider_error");
        }
        assert!(!error.to_string().contains("TAIL-SENTINEL"));
        assert_eq!(
            provider_error(b"broken-json sk-fixture-PREFIX/TAIL-SENTINEL", &key),
            error
        );
    }

    #[test]
    fn store_read_only_when_owner_only_and_masked() {
        let d = std::env::temp_dir().join(format!("stratum-keys-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
        let p = d.join("keys.env");
        fs::write(&p, "# store\nexport OPENAI_API_KEY=\"fixture-value-SENTINEL-1\"\nANTHROPIC_API_KEY=fixture-two-SENTINEL\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            read(&p, "OPENAI_API_KEY").is_err(),
            "a wider mode is refused"
        );
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let k = read(&p, "OPENAI_API_KEY").unwrap();
        assert_eq!(k.expose(), "fixture-value-SENTINEL-1");
        assert_eq!(format!("{k:?} {k}"), "Secret(***) ***");
        assert_eq!(
            read(&p, "ANTHROPIC_API_KEY").unwrap().expose(),
            "fixture-two-SENTINEL"
        );
        assert!(read(&p, "OTHER").is_err());
        let m = mask(
            b"x-api-key: fixture-two-SENTINEL and fixture-value-SENTINEL-1",
            &[&k],
        );
        assert!(!String::from_utf8_lossy(&m).contains("SENTINEL"));
        let _ = fs::remove_file(&p);
        let _ = fs::remove_dir(&d);
    }
}
