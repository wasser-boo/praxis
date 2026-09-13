use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub user_id: String,
    pub fact: String,
    pub category: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Memory {
    pub custom_variables: std::collections::HashMap<String, serde_json::Value>,
    pub learned_facts: Vec<String>,
    pub last_topics: Vec<String>,
    pub user_preferences: std::collections::HashMap<String, serde_json::Value>,
}

impl Database {
    pub fn add_memory(
        &self,
        user_id: &str,
        fact: &str,
        category: Option<&str>,
    ) -> anyhow::Result<i64> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let category = canonical_category(category);
        if matches!(category, "preference" | "custom_variable") {
            let (key, value) = decode_pair(fact)?;
            let mut memory = load_with_legacy_preferences(&tx, user_id)?;
            if category == "preference" { memory.user_preferences.insert(key.clone(), value.clone()); }
            else { memory.custom_variables.insert(key.clone(), value.clone()); }
            save_to(&tx, user_id, &memory)?;
            let encoded = serde_json::to_string(&serde_json::json!({"key": key, "value": value}))?;
            let id = tx.query_row("SELECT id FROM memory WHERE user_id = ?1 AND category = ?2 AND fact = ?3", rusqlite::params![user_id, category, encoded], |row| row.get(0))?;
            tx.commit()?;
            return Ok(id);
        }
        // Equivalent retries return the actual existing ID, not an affected-row count.
        let existing = {
            use rusqlite::OptionalExtension;
            tx.query_row(
                "SELECT id FROM memory WHERE user_id = ?1 AND fact = ?2 AND COALESCE(category, 'learned_fact') IN (?3, ?4) ORDER BY id LIMIT 1",
                rusqlite::params![user_id, fact, category, if category == "learned_fact" { "fact" } else { category }],
                |row| row.get::<_, i64>(0),
            ).optional()?
        };
        let id = if let Some(id) = existing { id } else {
            tx.execute("INSERT INTO memory (user_id, fact, category) VALUES (?1, ?2, ?3)", rusqlite::params![user_id, fact, category])?;
            tx.last_insert_rowid()
        };
        tx.commit()?;
        Ok(id)
    }

    pub fn get_memories(&self, user_id: &str, limit: i32) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, fact, category, created_at FROM memory WHERE user_id = ?1 AND COALESCE(category, '') != '_praxis_memory_v2' ORDER BY id DESC LIMIT ?2"
        )?;

        let entries = stmt
            .query_map(rusqlite::params![user_id, limit], |row| {
                Ok(MemoryEntry {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    fact: row.get(2)?,
                    category: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries.into_iter().rev().collect())
    }

    pub fn delete_memory(&self, user_id: &str, id: i64) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM memory WHERE user_id = ?1 AND id = ?2", rusqlite::params![user_id, id])?;
        Ok(())
    }
}

fn canonical_category(category: Option<&str>) -> &str {
    match category { None | Some("fact") => "learned_fact", Some(other) => other }
}

fn entries(conn: &rusqlite::Connection, user_id: &str) -> anyhow::Result<Vec<(i64, String, String)>> {
    let mut stmt = conn.prepare("SELECT id, fact, category FROM memory WHERE user_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([user_id], |row| {
        let category: Option<String> = row.get(2)?;
        Ok((row.get(0)?, row.get(1)?, canonical_category(category.as_deref()).to_string()))
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn decode_pair(entry: &str) -> anyhow::Result<(String, serde_json::Value)> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(entry) {
        if let (Some(key), Some(value)) = (value.get("key").and_then(|v| v.as_str()), value.get("value")) {
            return Ok((key.to_string(), value.clone()));
        }
    }
    // Legacy string representation; split once so values containing '=' survive.
    let (key, value) = entry.split_once('=').ok_or_else(|| anyhow::anyhow!("Invalid stored memory key/value entry"))?;
    Ok((key.to_string(), serde_json::json!(value)))
}

fn load_from(conn: &rusqlite::Connection, user_id: &str) -> anyhow::Result<Memory> {
    let mut memory = Memory::default();
    // Read all records, oldest first: latest preference wins, older keys aren't
    // silently lost when more than 100 facts have been learned.
    for (_, fact, category) in entries(conn, user_id)? {
        match category.as_str() {
            "learned_fact" => add_learned_fact(&mut memory, &fact),
            "topic" => add_topic(&mut memory, &fact),
            "preference" => { let (key, value) = decode_pair(&fact)?; memory.user_preferences.insert(key, value); }
            "custom_variable" => { let (key, value) = decode_pair(&fact)?; memory.custom_variables.insert(key, value); }
            _ => {}
        }
    }
    Ok(memory)
}

fn mark_legacy_imported(conn: &rusqlite::Connection, user_id: &str) -> anyhow::Result<()> {
    conn.execute("INSERT INTO memory (user_id, fact, category) SELECT ?1, 'context_preferences_imported', '_praxis_memory_v2' WHERE NOT EXISTS (SELECT 1 FROM memory WHERE user_id = ?1 AND category = '_praxis_memory_v2')", [user_id])?;
    Ok(())
}

fn load_with_legacy_preferences(conn: &rusqlite::Connection, user_id: &str) -> anyhow::Result<Memory> {
    use rusqlite::OptionalExtension;
    let mut memory = load_from(conn, user_id)?;
    let imported: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM memory WHERE user_id = ?1 AND category = '_praxis_memory_v2')", [user_id], |row| row.get(0))?;
    if imported { return Ok(memory); }
    let data: Option<String> = conn.query_row("SELECT data FROM contexts WHERE user_id = ?1", [user_id], |row| row.get(0)).optional()?;
    if let Some(data) = data {
        let mut context: serde_json::Value = serde_json::from_str(&data)?;
        if let Some(session) = context.get("session_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty() && *s != "default") {
            let key = format!("{user_id}:::{session}");
            let data: Option<String> = conn.query_row("SELECT data FROM contexts WHERE user_id = ?1", [key], |row| row.get(0)).optional()?;
            if let Some(data) = data { context = serde_json::from_str(&data)?; }
        }
        let mut found = false;
        if let Some(custom) = context.get("custom_data").and_then(|v| v.as_object()) {
            for (key, value) in custom {
                if let Some(key) = key.strip_prefix("pref_") {
                    memory.user_preferences.entry(key.to_string()).or_insert(value.clone());
                    found = true;
                }
            }
        }
        if found {
            save_to(conn, user_id, &memory)?;
            mark_legacy_imported(conn, user_id)?;
        }
    }
    Ok(memory)
}

/// Load exactly this user's durable memory. Legacy pref_* values are imported
/// once, without overriding current durable preferences or resurrecting clears.
pub fn load_memory(db: &Database, user_id: &str) -> anyhow::Result<Memory> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    let memory = load_with_legacy_preferences(&tx, user_id)?;
    tx.commit()?;
    Ok(memory)
}

fn save_to(conn: &rusqlite::Connection, user_id: &str, memory: &Memory) -> anyhow::Result<()> {
    // Deterministic desired snapshot; unchanged rows retain IDs/timestamps.
    let mut desired = Vec::new();
    for fact in &memory.learned_facts { desired.push(("learned_fact".to_string(), fact.clone())); }
    for topic in &memory.last_topics { desired.push(("topic".to_string(), topic.clone())); }
    for (category, map) in [("preference", &memory.user_preferences), ("custom_variable", &memory.custom_variables)] {
        let mut keys: Vec<_> = map.keys().collect();
        keys.sort();
        for key in keys {
            desired.push((category.to_string(), serde_json::to_string(&serde_json::json!({"key": key, "value": map[key]}))?));
        }
    }
    let mut seen = std::collections::HashSet::new();
    desired.retain(|entry| seen.insert(entry.clone()));
    let wanted: std::collections::HashSet<_> = desired.iter().cloned().collect();
    let mut retained = std::collections::HashSet::new();
    for (id, fact, category) in entries(conn, user_id)? {
        if !matches!(category.as_str(), "learned_fact" | "topic" | "preference" | "custom_variable") { continue; }
        let entry = (category, fact);
        if !wanted.contains(&entry) || !retained.insert(entry) {
            conn.execute("DELETE FROM memory WHERE user_id = ?1 AND id = ?2", rusqlite::params![user_id, id])?;
        }
    }
    for (category, fact) in desired {
        if !retained.contains(&(category.clone(), fact.clone())) {
            conn.execute("INSERT INTO memory (user_id, fact, category) VALUES (?1, ?2, ?3)", rusqlite::params![user_id, fact, category])?;
        }
    }
    Ok(())
}

/// Replace managed memory atomically. Empty collections really clear entries;
/// unknown categories are preserved, and retries do not append duplicates.
pub fn save_memory(db: &Database, user_id: &str, memory: &Memory) -> anyhow::Result<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    save_to(&tx, user_id, memory)?;
    mark_legacy_imported(&tx, user_id)?;
    tx.commit()?;
    Ok(())
}

/// Atomic read/modify/write for runtime and partial dashboard updates.
pub fn update_memory(
    db: &Database,
    user_id: &str,
    update: impl FnOnce(&mut Memory),
) -> anyhow::Result<Memory> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    let mut memory = load_with_legacy_preferences(&tx, user_id)?;
    update(&mut memory);
    save_to(&tx, user_id, &memory)?;
    mark_legacy_imported(&tx, user_id)?;
    tx.commit()?;
    Ok(memory)
}

/// Add a learned fact to memory
pub fn add_learned_fact(memory: &mut Memory, fact: &str) {
    if !memory.learned_facts.contains(&fact.to_string()) {
        memory.learned_facts.push(fact.to_string());
    }
}

/// Update a user preference
pub fn update_preference(memory: &mut Memory, key: &str, value: &serde_json::Value) {
    memory
        .user_preferences
        .insert(key.to_string(), value.clone());
}

/// Add a topic to memory
pub fn add_topic(memory: &mut Memory, topic: &str) {
    if !memory.last_topics.contains(&topic.to_string()) {
        memory.last_topics.push(topic.to_string());
    }
}

/// Update custom variables in memory
pub fn update_custom_variables(
    memory: &mut Memory,
    updates: &std::collections::HashMap<String, serde_json::Value>,
) {
    for (key, value) in updates {
        memory.custom_variables.insert(key.clone(), value.clone());
    }
}

/// Delete custom variables from memory
pub fn delete_custom_variables(memory: &mut Memory, keys: &[String]) {
    for key in keys {
        memory.custom_variables.remove(key);
    }
}

/// Load variables from memory and merge with context custom_data
pub fn load_variables(
    db: &Database,
    user_id: &str,
) -> anyhow::Result<std::collections::HashMap<String, serde_json::Value>> {
    let memory = load_memory(db, user_id)?;
    let mut variables = std::collections::HashMap::new();

    // Load from custom_variables
    for (key, value) in &memory.custom_variables {
        variables.insert(key.clone(), value.clone());
    }

    // Load from user_preferences
    for (key, value) in &memory.user_preferences {
        variables.insert(key.clone(), value.clone());
    }

    Ok(variables)
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        (db, dir)
    }

    #[test]
    fn test_add_and_get_memory() {
        let (db, _dir) = test_db();
        let id = db
            .add_memory("user1", "User likes Rust", Some("preferences"))
            .unwrap();
        assert!(id > 0);
        let memories = db.get_memories("user1", 10).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].fact, "User likes Rust");
    }

    #[test]
    fn test_delete_memory() {
        let (db, _dir) = test_db();
        let id = db.add_memory("user1", "fact", None).unwrap();
        db.delete_memory("user1", id).unwrap();
        let memories = db.get_memories("user1", 10).unwrap();
        assert!(memories.is_empty());
    }

    #[test]
    fn backend_memory_ids_deduplication_and_user_isolation() {
        let (db, _dir) = test_db();
        let alice = db.add_memory("alice", "same fact", Some("fact")).unwrap();
        let bob = db.add_memory("bob", "same fact", Some("fact")).unwrap();
        assert_ne!(alice, bob);
        assert_eq!(alice, db.add_memory("alice", "same fact", Some("learned_fact")).unwrap());
        db.delete_memory("bob", alice).unwrap();
        assert_eq!(db.get_memories("alice", 100).unwrap().len(), 1);
        assert_eq!(db.get_memories("bob", 100).unwrap().len(), 1);
    }

    #[test]
    fn backend_memory_snapshot_is_typed_idempotent_and_replaceable() {
        let (db, _dir) = test_db();
        let mut memory = Memory::default();
        memory.learned_facts = vec!["one".into(), "one".into()];
        memory.user_preferences.insert("answer=style".into(), serde_json::json!({"brief": true, "n": 2}));
        memory.custom_variables.insert("list".into(), serde_json::json!([1, false, null]));
        save_memory(&db, "alice", &memory).unwrap();
        let ids: Vec<_> = db.get_memories("alice", 100).unwrap().iter().map(|e| e.id).collect();
        save_memory(&db, "alice", &memory).unwrap();
        assert_eq!(ids, db.get_memories("alice", 100).unwrap().iter().map(|e| e.id).collect::<Vec<_>>());
        let loaded = load_memory(&db, "alice").unwrap();
        assert_eq!(loaded.learned_facts, vec!["one"]);
        assert_eq!(loaded.user_preferences, memory.user_preferences);
        assert_eq!(loaded.custom_variables, memory.custom_variables);
        save_memory(&db, "alice", &Memory::default()).unwrap();
        assert!(load_memory(&db, "alice").unwrap().learned_facts.is_empty());
        assert!(db.get_memories("alice", 100).unwrap().is_empty());
    }

    #[test]
    fn backend_legacy_context_preferences_import_once_and_stay_cleared() {
        let (db, _dir) = test_db();
        db.merge_context("alice", serde_json::json!({"custom_data.pref_theme": "old", "custom_data.keep": true})).unwrap();
        assert_eq!(load_memory(&db, "alice").unwrap().user_preferences["theme"], "old");
        update_memory(&db, "alice", |m| { m.user_preferences.insert("theme".into(), serde_json::json!("new")); }).unwrap();
        assert_eq!(load_memory(&db, "alice").unwrap().user_preferences["theme"], "new");
        update_memory(&db, "alice", |m| m.user_preferences.clear()).unwrap();
        assert!(load_memory(&db, "alice").unwrap().user_preferences.is_empty());
        assert!(load_memory(&db, "bob").unwrap().user_preferences.is_empty());
        assert_eq!(db.load_context("alice").unwrap().custom_data["keep"], true);
    }

    #[test]
    fn backend_preference_revisions_can_return_to_an_older_value() {
        let (db, _dir) = test_db();
        for value in ["theme=dark", "theme=light", "theme=dark"] {
            db.add_memory("alice", value, Some("preference")).unwrap();
        }
        assert_eq!(load_memory(&db, "alice").unwrap().user_preferences["theme"], "dark");
        assert_eq!(db.get_memories("alice", 100).unwrap().len(), 1);
    }

    #[test]
    fn backend_memory_legacy_latest_wins_and_no_hundred_row_loss() {
        let (db, _dir) = test_db();
        {
            let conn = db.conn();
            for entry in ["theme=dark", "theme=light", "equation=a=b"] {
                conn.execute("INSERT INTO memory (user_id, fact, category) VALUES ('alice', ?1, 'preference')", [entry]).unwrap();
            }
        }
        for n in 0..105 { db.add_memory("alice", &format!("fact {n}"), Some("fact")).unwrap(); }
        let memory = load_memory(&db, "alice").unwrap();
        assert_eq!(memory.user_preferences["theme"], "light");
        assert_eq!(memory.user_preferences["equation"], "a=b");
        assert_eq!(memory.learned_facts.len(), 105);
        assert!(load_memory(&db, "bob").unwrap().learned_facts.is_empty());
    }

    #[test]
    fn backend_memory_concurrent_updates_do_not_overwrite_each_other() {
        let (db, _dir) = test_db();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let threads: Vec<_> = (0..4).map(|n| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_memory(&db, "alice", |m| { m.custom_variables.insert(format!("key{n}"), serde_json::json!(n)); }).unwrap();
            })
        }).collect();
        for thread in threads { thread.join().unwrap(); }
        assert_eq!(load_memory(&db, "alice").unwrap().custom_variables.len(), 4);
    }

    #[test]
    fn backend_memory_failure_rolls_back_and_reads_propagate_errors() {
        let (db, _dir) = test_db();
        db.add_memory("alice", "keep", Some("fact")).unwrap();
        db.conn().execute_batch("CREATE TRIGGER reject_memory BEFORE INSERT ON memory WHEN NEW.fact = 'reject' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        let mut replacement = Memory::default();
        replacement.learned_facts.push("reject".into());
        assert!(save_memory(&db, "alice", &replacement).is_err());
        assert_eq!(load_memory(&db, "alice").unwrap().learned_facts, vec!["keep"]);
        db.conn().execute_batch("DROP TABLE memory;").unwrap();
        assert!(load_memory(&db, "alice").is_err());
    }

    #[test]
    fn test_memory_struct_default() {
        let memory = Memory::default();
        assert!(memory.learned_facts.is_empty());
        assert!(memory.last_topics.is_empty());
        assert!(memory.user_preferences.is_empty());
        assert!(memory.custom_variables.is_empty());
    }

    #[test]
    fn test_add_learned_fact() {
        let mut memory = Memory::default();
        add_learned_fact(&mut memory, "User prefers Rust");
        assert_eq!(memory.learned_facts.len(), 1);
        assert_eq!(memory.learned_facts[0], "User prefers Rust");
    }

    #[test]
    fn test_add_learned_fact_no_duplicates() {
        let mut memory = Memory::default();
        add_learned_fact(&mut memory, "fact1");
        add_learned_fact(&mut memory, "fact1");
        assert_eq!(memory.learned_facts.len(), 1);
    }

    #[test]
    fn test_update_preference() {
        let mut memory = Memory::default();
        update_preference(&mut memory, "theme", &serde_json::json!("dark"));
        assert_eq!(memory.user_preferences["theme"], "dark");
    }

    #[test]
    fn test_add_topic() {
        let mut memory = Memory::default();
        add_topic(&mut memory, "Rust async");
        assert_eq!(memory.last_topics.len(), 1);
        assert_eq!(memory.last_topics[0], "Rust async");
    }

    #[test]
    fn test_add_topic_no_duplicates() {
        let mut memory = Memory::default();
        add_topic(&mut memory, "topic1");
        add_topic(&mut memory, "topic1");
        assert_eq!(memory.last_topics.len(), 1);
    }

    #[test]
    fn test_update_custom_variables() {
        let mut memory = Memory::default();
        let mut updates = std::collections::HashMap::new();
        updates.insert("lang".to_string(), serde_json::json!("rust"));
        update_custom_variables(&mut memory, &updates);
        assert_eq!(memory.custom_variables["lang"], "rust");
    }

    #[test]
    fn test_delete_custom_variables() {
        let mut memory = Memory::default();
        memory
            .custom_variables
            .insert("a".to_string(), serde_json::json!("1"));
        memory
            .custom_variables
            .insert("b".to_string(), serde_json::json!("2"));
        delete_custom_variables(&mut memory, &["a".to_string()]);
        assert!(!memory.custom_variables.contains_key("a"));
        assert!(memory.custom_variables.contains_key("b"));
    }

    #[test]
    fn test_load_save_memory() {
        let (db, _dir) = test_db();
        let mut memory = Memory::default();
        add_learned_fact(&mut memory, "fact1");
        add_topic(&mut memory, "topic1");
        save_memory(&db, "user1", &memory).unwrap();

        let loaded = load_memory(&db, "user1").unwrap();
        assert!(loaded.learned_facts.contains(&"fact1".to_string()));
        assert!(loaded.last_topics.contains(&"topic1".to_string()));
    }
}

/// Record TTS/media character usage into memory.variables.media_spend.
/// Structure: {"tts_chars_total": N, "tts_chars_month_YYYY-MM": n, "tts_calls": n}
pub fn record_media_spend(db: &Database, user_id: &str, chars: usize) -> anyhow::Result<()> {
    update_memory(db, user_id, |memory| {
        let month = chrono::Utc::now().format("%Y-%m").to_string();
        let entry = memory
            .custom_variables
            .entry("media_spend".to_string())
            .or_insert_with(|| serde_json::json!({}));
        let obj = match entry.as_object_mut() {
            Some(o) => o,
            None => return,
        };
        *obj.entry("tts_chars_total".to_string())
            .or_insert(serde_json::json!(0)) = serde_json::json!(
                obj.get("tts_chars_total").and_then(|v| v.as_u64()).unwrap_or(0) + chars as u64
            );
        let month_key = format!("tts_chars_month_{month}");
        *obj.entry(month_key).or_insert(serde_json::json!(0)) = serde_json::json!(
            obj.get(&month_key).and_then(|v| v.as_u64()).unwrap_or(0) + chars as u64
        );
        *obj.entry("tts_calls".to_string())
            .or_insert(serde_json::json!(0)) = serde_json::json!(
                obj.get("tts_calls").and_then(|v| v.as_u64()).unwrap_or(0) + 1
            );
    })?;
    Ok(())
}

/// Read the current media spend record (None when absent).
pub fn load_media_spend(db: &Database, user_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let memory = load_memory(db, user_id)?;
    Ok(memory.custom_variables.get("media_spend").cloned())
}
