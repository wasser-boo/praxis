pub mod contexts;
pub mod cron_jobs;
pub mod enc2;
pub mod logs;
pub mod memory;
pub mod messages;
pub mod pairings;
pub mod secrets;
pub mod templates;
pub mod tools;

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    pub data_dir: String,
}

impl Database {
    pub fn new(data_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir)?;

        let db_path = data_dir.join("praxis.db");
        let conn = Connection::open(&db_path)?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            data_dir: data_dir.to_string_lossy().to_string(),
        };

        db.run_migrations()?;
        Ok(db)
    }

    fn run_migrations(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();

        let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            conn.execute_batch(include_str!("../../migrations/001_initial.sql"))?;
            conn.pragma_update(None, "user_version", 1)?;
        }
        if version < 2 {
            conn.execute_batch(include_str!("../../migrations/002_cron_jobs.sql"))?;
            conn.pragma_update(None, "user_version", 2)?;
        }
        if version < 3 {
            conn.execute_batch(include_str!("../../migrations/003_vector_store.sql"))?;
            conn.pragma_update(None, "user_version", 3)?;
        }
        if version < 4 {
            conn.execute_batch(include_str!("../../migrations/004_tool_calls.sql"))?;
            conn.pragma_update(None, "user_version", 4)?;
        }

        Ok(())
    }

    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    pub fn data_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.data_dir)
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("SELECT 1")?;
        Ok(())
    }

    pub fn backup(&self, dest: &Path) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM INTO ?1", rusqlite::params![dest.to_string_lossy()])?;
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
    fn test_database_creation() {
        let (_db, _dir) = test_db();
    }

    #[test]
    fn test_database_ping() {
        let (db, _dir) = test_db();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(db.ping()).unwrap();
    }
}
