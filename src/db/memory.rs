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
        let conn = self.conn();
        let id = conn.execute(
            "INSERT INTO memory (user_id, fact, category) VALUES (?1, ?2, ?3)",
            rusqlite::params![user_id, fact, category],
        )?;
        Ok(id as i64)
    }

    pub fn get_memories(&self, user_id: &str, limit: i32) -> anyhow::Result<Vec<MemoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, fact, category, created_at FROM memory WHERE user_id = ?1 ORDER BY id DESC LIMIT ?2"
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

    pub fn delete_memory(&self, id: i64) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM memory WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }
}

/// Load memory for a user (aggregates from DB entries)
pub fn load_memory(db: &Database, user_id: &str) -> Memory {
    let mut memory = Memory::default();

    if let Ok(entries) = db.get_memories(user_id, 100) {
        for entry in &entries {
            match entry.category.as_deref() {
                Some("fact") | Some("learned_fact") => {
                    memory.learned_facts.push(entry.fact.clone());
                }
                Some("topic") => {
                    memory.last_topics.push(entry.fact.clone());
                }
                Some("preference") => {
                    if let Some(key) = entry.fact.splitn(2, '=').next() {
                        let val = entry.fact.splitn(2, '=').nth(1).unwrap_or("");
                        memory
                            .user_preferences
                            .insert(key.to_string(), serde_json::json!(val));
                    }
                }
                Some("custom_variable") => {
                    if let Some(key) = entry.fact.splitn(2, '=').next() {
                        let val = entry.fact.splitn(2, '=').nth(1).unwrap_or("");
                        memory
                            .custom_variables
                            .insert(key.to_string(), serde_json::json!(val));
                    }
                }
                _ => {}
            }
        }
    }

    memory
}

/// Save memory to DB
pub fn save_memory(db: &Database, user_id: &str, memory: &Memory) -> anyhow::Result<()> {
    // Save learned facts
    for fact in &memory.learned_facts {
        let _ = db.add_memory(user_id, fact, Some("learned_fact"));
    }

    // Save topics
    for topic in &memory.last_topics {
        let _ = db.add_memory(user_id, topic, Some("topic"));
    }

    // Save preferences
    for (key, val) in &memory.user_preferences {
        let entry = format!("{}={}", key, val.as_str().unwrap_or(""));
        let _ = db.add_memory(user_id, &entry, Some("preference"));
    }

    // Save custom variables
    for (key, val) in &memory.custom_variables {
        let entry = format!("{}={}", key, val.as_str().unwrap_or(""));
        let _ = db.add_memory(user_id, &entry, Some("custom_variable"));
    }

    Ok(())
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
) -> std::collections::HashMap<String, serde_json::Value> {
    let memory = load_memory(db, user_id);
    let mut variables = std::collections::HashMap::new();

    // Load from custom_variables
    for (key, value) in &memory.custom_variables {
        variables.insert(key.clone(), value.clone());
    }

    // Load from user_preferences
    for (key, value) in &memory.user_preferences {
        variables.insert(key.clone(), value.clone());
    }

    variables
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
        db.delete_memory(id).unwrap();
        let memories = db.get_memories("user1", 10).unwrap();
        assert!(memories.is_empty());
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

        let loaded = load_memory(&db, "user1");
        assert!(loaded.learned_facts.contains(&"fact1".to_string()));
        assert!(loaded.last_topics.contains(&"topic1".to_string()));
    }
}
