use serde::{Deserialize, Serialize};
use super::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: i64,
    pub user_id: String,
    pub fact: String,
    pub category: Option<String>,
    pub created_at: Option<String>,
}

impl Database {
    pub fn add_memory(&self, user_id: &str, fact: &str, category: Option<&str>) -> anyhow::Result<i64> {
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
        let id = db.add_memory("user1", "User likes Rust", Some("preferences")).unwrap();
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
}
