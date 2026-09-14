//! Typed durable variables surfaced by Dashboard → Memory → Custom Data.
//! The authenticated task supplies the user; arguments cannot select another user.
use crate::db::{memory, Database};
use serde_json::{json, Value};

fn key(args: &Value) -> anyhow::Result<&str> {
    let key = args["key"].as_str().ok_or_else(|| anyhow::anyhow!("key must be a string"))?;
    anyhow::ensure!(!key.is_empty() && key.len() <= 128 && key.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)),
        "key must contain 1..128 ASCII letters, digits, underscores, dots or hyphens");
    Ok(key)
}

pub fn get(db: &Database, user: &str, args: &Value) -> anyhow::Result<String> {
    anyhow::ensure!(crate::db::tools::get(db, "memory_get")?.is_enabled, "memory_get is disabled");
    let key = key(args)?;
    let memory = memory::load_memory(db, user)?;
    let value = memory.custom_variables.get(key);
    Ok(json!({"key":key, "exists":value.is_some(), "value":value}).to_string())
}

pub fn set(db: &Database, user: &str, args: &Value) -> anyhow::Result<String> {
    anyhow::ensure!(crate::db::tools::get(db, "memory_set")?.is_enabled, "memory_set is disabled");
    let key = key(args)?;
    let value = args.get("value").ok_or_else(|| anyhow::anyhow!("value is required"))?;
    anyhow::ensure!(serde_json::to_vec(value)?.len() <= 65536, "Memory variable exceeds 64 KiB");
    let mut conflict = false;
    memory::update_memory(db, user, |memory| {
        let current = memory.custom_variables.get(key).unwrap_or(&Value::Null);
        // A retry of an already-applied write is idempotent, including XP.
        if current == value { return; }
        if args.get("expected_value").is_some_and(|expected| expected != current) {
            conflict = true;
            return;
        }
        if value.is_null() { memory.custom_variables.remove(key); }
        else { memory.custom_variables.insert(key.to_string(), value.clone()); }
    })?;
    anyhow::ensure!(!conflict, "Memory changed since read; use memory_get and reconcile before retrying");
    Ok(json!({"saved":true, "key":key, "value":value, "deleted":value.is_null()}).to_string())
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
