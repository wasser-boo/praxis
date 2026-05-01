use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: i64,
    pub level: String,
    pub message: String,
    pub context: Option<String>,
    pub created_at: Option<String>,
}

impl Database {
    pub fn add_log(
        &self,
        level: &str,
        message: &str,
        context: Option<&str>,
    ) -> anyhow::Result<i64> {
        let conn = self.conn();
        let id = conn.execute(
            "INSERT INTO logs (level, message, context) VALUES (?1, ?2, ?3)",
            rusqlite::params![level, message, context],
        )?;
        Ok(id as i64)
    }

    pub fn get_logs(&self, limit: i32) -> anyhow::Result<Vec<LogEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, level, message, context, created_at FROM logs ORDER BY id DESC LIMIT ?1",
        )?;

        let entries = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(LogEntry {
                    id: row.get(0)?,
                    level: row.get(1)?,
                    message: row.get(2)?,
                    context: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    pub fn clear_logs(&self) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM logs", [])?;
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
    fn test_add_and_get_logs() {
        let (db, _dir) = test_db();
        db.add_log("info", "Test message", Some("test")).unwrap();
        let logs = db.get_logs(10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "info");
    }

    #[test]
    fn test_clear_logs() {
        let (db, _dir) = test_db();
        db.add_log("info", "msg", None).unwrap();
        db.clear_logs().unwrap();
        let logs = db.get_logs(10).unwrap();
        assert!(logs.is_empty());
    }
}
