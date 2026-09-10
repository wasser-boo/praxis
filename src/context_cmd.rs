//! Shared parser for the `/context` slash command supported across the TUI,
//! the web chat, and Discord.
//!
//! Syntax (whitespace-separated):
//!
//! ```text
//! set    key=value [key=value ...]    Merge into context (dot-notation supported)
//! get    key                          Read a single value
//! unset  key                          Remove a key
//! show   [path]                       Pretty-print all (or a sub-path of) context
//! ```
//!
//! Values are parsed permissively:
//!
//! - `true` / `false`        → boolean
//! - `null`                  → null
//! - All-digits / `-?\d+(\.\d+)?` → number
//! - Starts with `{`, `[`, `"` → JSON literal (must parse cleanly)
//! - Anything else            → string
//!
//! Keys support dot-notation (e.g. `custom_data.device`,
//! `settings.voice_tts_enabled`). The dotted form is forwarded verbatim to
//! [`Database::merge_context`], which performs a deep-merge that creates
//! intermediate objects as needed.

use crate::db::Database;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ContextOp {
    Set(serde_json::Map<String, Value>),
    Get(String),
    Unset(String),
    Show(Option<String>),
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("usage: /context <set|get|unset|show> ...")]
    MissingSubcommand,
    #[error("usage: /context set key=value [key=value ...]")]
    SetMissingArgs,
    #[error("invalid pair `{0}` — expected key=value")]
    BadPair(String),
    #[error("usage: /context get <key>")]
    GetMissingKey,
    #[error("usage: /context unset <key>")]
    UnsetMissingKey,
    #[error("invalid JSON value: {0}")]
    BadJson(String),
    #[error("unknown subcommand `{0}`")]
    UnknownSubcommand(String),
}

/// Parse a `/context …` slash command body. The leading `/context` is
/// optional and stripped if present.
pub fn parse(input: &str) -> Result<ContextOp, ParseError> {
    let trimmed = input.trim();
    let trimmed = trimmed
        .strip_prefix("/context")
        .or_else(|| trimmed.strip_prefix("/ctx"))
        .unwrap_or(trimmed)
        .trim();
    let mut parts = trimmed.split_whitespace();
    let sub = parts.next().ok_or(ParseError::MissingSubcommand)?;
    match sub.to_ascii_lowercase().as_str() {
        "set" => {
            // Re-tokenize the remainder: pairs may contain spaces inside JSON
            // values, so split on whitespace OUTSIDE of quotes/brackets.
            let rest = trimmed[sub.len()..].trim();
            if rest.is_empty() {
                return Err(ParseError::SetMissingArgs);
            }
            let pairs = split_pairs(rest);
            if pairs.is_empty() {
                return Err(ParseError::SetMissingArgs);
            }
            let mut map = serde_json::Map::new();
            for pair in pairs {
                let (k, v) = pair
                    .split_once('=')
                    .ok_or_else(|| ParseError::BadPair(pair.clone()))?;
                let key = k.trim().to_string();
                if key.is_empty() {
                    return Err(ParseError::BadPair(pair.clone()));
                }
                let value = parse_value(v.trim())?;
                map.insert(key, value);
            }
            Ok(ContextOp::Set(map))
        }
        "get" => {
            let key = parts.next().ok_or(ParseError::GetMissingKey)?.to_string();
            Ok(ContextOp::Get(key))
        }
        "unset" | "del" | "delete" | "rm" => {
            let key = parts.next().ok_or(ParseError::UnsetMissingKey)?.to_string();
            Ok(ContextOp::Unset(key))
        }
        "show" | "list" | "ls" => {
            let path = parts.next().map(|s| s.to_string());
            Ok(ContextOp::Show(path))
        }
        other => Err(ParseError::UnknownSubcommand(other.to_string())),
    }
}

/// Split the argument list of `set` on whitespace, but treat content inside
/// matched `"…"`, `{…}`, or `[…]` as opaque so values that contain spaces
/// (e.g. JSON objects) survive intact.
fn split_pairs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut prev_backslash = false;
    for ch in s.chars() {
        if in_str {
            current.push(ch);
            if ch == '"' && !prev_backslash {
                in_str = false;
            }
            prev_backslash = ch == '\\' && !prev_backslash;
            continue;
        }
        prev_backslash = false;
        match ch {
            '"' => {
                in_str = true;
                current.push(ch);
            }
            '{' | '[' => {
                depth += 1;
                current.push(ch);
            }
            '}' | ']' => {
                depth -= 1;
                current.push(ch);
            }
            c if c.is_whitespace() && depth == 0 => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Parse a single value with permissive type inference.
pub fn parse_value(raw: &str) -> Result<Value, ParseError> {
    if raw.is_empty() {
        return Ok(Value::String(String::new()));
    }
    match raw {
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        "null" => return Ok(Value::Null),
        _ => {}
    }
    if let Ok(n) = raw.parse::<i64>() {
        return Ok(Value::Number(n.into()));
    }
    if let Ok(f) = raw.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(f) {
            return Ok(Value::Number(num));
        }
    }
    let first = raw.chars().next().unwrap();
    if matches!(first, '{' | '[' | '"') {
        return serde_json::from_str(raw)
            .map_err(|e| ParseError::BadJson(format!("{e}: `{raw}`")));
    }
    Ok(Value::String(raw.to_string()))
}

/// Apply a parsed [`ContextOp`] against the local database. Returns a
/// human-readable response string suitable for echoing back to the user.
pub fn apply(db: &Database, user_id: &str, op: &ContextOp) -> String {
    match op {
        ContextOp::Set(map) => {
            let updates = Value::Object(map.clone());
            match db.merge_context(user_id, updates) {
                Ok(_) => {
                    let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
                    format!("✓ context updated: {}", keys.join(", "))
                }
                Err(e) => format!("✗ context update failed: {e}"),
            }
        }
        ContextOp::Get(key) => match db.load_context(user_id) {
            Ok(ctx) => {
                let val = serde_json::to_value(&ctx).unwrap_or(Value::Null);
                let v = lookup(&val, key).unwrap_or(Value::Null);
                format!("{key} = {}", serde_json::to_string_pretty(&v).unwrap_or_default())
            }
            Err(e) => format!("load failed: {e}"),
        },
        ContextOp::Unset(key) => {
            // Implement unset by merging with `null`; the consumer is expected
            // to treat `null` as absent. We don't actually delete keys from
            // the JSON because removing them needs a different DB primitive.
            let updates = serde_json::json!({ key: Value::Null });
            match db.merge_context(user_id, updates) {
                Ok(_) => format!("✓ {key} cleared"),
                Err(e) => format!("✗ {e}"),
            }
        }
        ContextOp::Show(path) => match db.load_context(user_id) {
            Ok(ctx) => {
                let val = serde_json::to_value(&ctx).unwrap_or(Value::Null);
                let view = match path.as_deref() {
                    Some(p) => lookup(&val, p).unwrap_or(Value::Null),
                    None => val,
                };
                serde_json::to_string_pretty(&view).unwrap_or_default()
            }
            Err(e) => format!("load failed: {e}"),
        },
    }
}

/// Look up a (possibly dotted) path inside a JSON value.
fn lookup(val: &Value, path: &str) -> Option<Value> {
    let path = crate::db::contexts::canonical_context_key(path);
    let mut current = val;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_simple() {
        let op = parse("/context set foo=bar").unwrap();
        match op {
            ContextOp::Set(m) => {
                assert_eq!(m.get("foo"), Some(&Value::String("bar".into())));
            }
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn parse_set_dotted_keys_and_types() {
        let op = parse("/ctx set custom_data.device=main settings.max_llm_turns=20 settings.voice_tts_enabled=true").unwrap();
        let ContextOp::Set(m) = op else { panic!() };
        assert_eq!(m.get("custom_data.device").unwrap(), &Value::String("main".into()));
        assert_eq!(m.get("settings.max_llm_turns").unwrap(), &Value::Number(20.into()));
        assert_eq!(m.get("settings.voice_tts_enabled").unwrap(), &Value::Bool(true));
    }

    #[test]
    fn parse_set_json_value() {
        let op = parse(r#"/context set custom_data.target={"foo":"bar","n":1}"#).unwrap();
        let ContextOp::Set(m) = op else { panic!() };
        assert_eq!(m["custom_data.target"]["foo"], Value::String("bar".into()));
        assert_eq!(m["custom_data.target"]["n"], Value::Number(1.into()));
    }

    #[test]
    fn parse_set_quoted_string_with_spaces() {
        let op = parse(r#"/context set custom_data.note="hello world""#).unwrap();
        let ContextOp::Set(m) = op else { panic!() };
        assert_eq!(m["custom_data.note"], Value::String("hello world".into()));
    }

    #[test]
    fn parse_get_unset_show() {
        assert!(matches!(parse("/context get foo").unwrap(), ContextOp::Get(_)));
        assert!(matches!(parse("/context unset foo").unwrap(), ContextOp::Unset(_)));
        assert!(matches!(parse("/context show").unwrap(), ContextOp::Show(None)));
        assert!(matches!(parse("/context show settings").unwrap(), ContextOp::Show(Some(_))));
    }

    #[test]
    fn parse_errors() {
        assert!(matches!(parse("/context").unwrap_err(), ParseError::MissingSubcommand));
        assert!(matches!(parse("/context set").unwrap_err(), ParseError::SetMissingArgs));
        assert!(matches!(parse("/context set bareword").unwrap_err(), ParseError::BadPair(_)));
        assert!(matches!(parse("/context bork").unwrap_err(), ParseError::UnknownSubcommand(_)));
    }

    #[test]
    fn apply_set_and_get_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let op = parse("/context set custom_data.device=main settings.max_llm_turns=42").unwrap();
        let _ = apply(&db, "alice", &op);
        let ctx = db.load_context("alice").unwrap();
        assert_eq!(ctx.custom_data["device"], Value::String("main".into()));
        assert_eq!(ctx.settings.max_llm_turns, Some(42));
    }
}
