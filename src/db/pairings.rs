use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pairing {
    pub user_id: String,
    pub discord_user_id: String,
    pub discord_guild_id: Option<String>,
    pub paired_at: Option<String>,
    pub last_seen_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPairing {
    pub code: String,
    pub discord_user_id: String,
    pub expires_at: String,
    pub created_at: Option<String>,
}

impl Database {
    pub fn create_pairing(
        &self,
        user_id: &str,
        discord_user_id: &str,
        discord_guild_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO pairings (user_id, discord_user_id, discord_guild_id, paired_at) VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![user_id, discord_user_id, discord_guild_id],
        )?;
        Ok(())
    }

    pub fn get_pairing_by_discord(&self, discord_user_id: &str) -> anyhow::Result<Option<Pairing>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT user_id, discord_user_id, discord_guild_id, paired_at, last_seen_at FROM pairings WHERE discord_user_id = ?1",
            rusqlite::params![discord_user_id],
            |row| {
                Ok(Pairing {
                    user_id: row.get(0)?,
                    discord_user_id: row.get(1)?,
                    discord_guild_id: row.get(2)?,
                    paired_at: row.get(3)?,
                    last_seen_at: row.get(4)?,
                })
            },
        );

        match result {
            Ok(pairing) => Ok(Some(pairing)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_pairing_by_internal_user(&self, user_id: &str) -> anyhow::Result<Option<Pairing>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT user_id, discord_user_id, discord_guild_id, paired_at, last_seen_at FROM pairings WHERE user_id = ?1",
            rusqlite::params![user_id],
            |row| {
                Ok(Pairing {
                    user_id: row.get(0)?,
                    discord_user_id: row.get(1)?,
                    discord_guild_id: row.get(2)?,
                    paired_at: row.get(3)?,
                    last_seen_at: row.get(4)?,
                })
            },
        );

        match result {
            Ok(pairing) => Ok(Some(pairing)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn update_last_seen(&self, discord_user_id: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE pairings SET last_seen_at = datetime('now') WHERE discord_user_id = ?1",
            rusqlite::params![discord_user_id],
        )?;
        Ok(())
    }

    pub fn create_pending_pairing(
        &self,
        code: &str,
        discord_user_id: &str,
        expires_at: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO pending_pairings (code, discord_user_id, expires_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![code, discord_user_id, expires_at],
        )?;
        Ok(())
    }

    pub fn get_pending_pairing(&self, code: &str) -> anyhow::Result<Option<PendingPairing>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT code, discord_user_id, expires_at, created_at FROM pending_pairings WHERE code = ?1",
            rusqlite::params![code],
            |row| {
                Ok(PendingPairing {
                    code: row.get(0)?,
                    discord_user_id: row.get(1)?,
                    expires_at: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        );

        match result {
            Ok(pairing) => Ok(Some(pairing)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete_pending_pairing(&self, code: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM pending_pairings WHERE code = ?1",
            rusqlite::params![code],
        )?;
        Ok(())
    }

    pub fn list_all_pairings(&self) -> anyhow::Result<Vec<Pairing>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT user_id, discord_user_id, discord_guild_id, paired_at, last_seen_at FROM pairings ORDER BY paired_at DESC"
        )?;
        let pairings = stmt
            .query_map([], |row| {
                Ok(Pairing {
                    user_id: row.get(0)?,
                    discord_user_id: row.get(1)?,
                    discord_guild_id: row.get(2)?,
                    paired_at: row.get(3)?,
                    last_seen_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pairings)
    }

    pub fn list_all_pending_pairings(&self) -> anyhow::Result<Vec<PendingPairing>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT code, discord_user_id, expires_at, created_at FROM pending_pairings ORDER BY created_at DESC"
        )?;
        let pending = stmt
            .query_map([], |row| {
                Ok(PendingPairing {
                    code: row.get(0)?,
                    discord_user_id: row.get(1)?,
                    expires_at: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(pending)
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
    fn test_create_and_get_pairing() {
        let (db, _dir) = test_db();
        db.create_pairing("user1", "discord1", Some("guild1"))
            .unwrap();
        let pairing = db.get_pairing_by_discord("discord1").unwrap().unwrap();
        assert_eq!(pairing.user_id, "user1");
        assert_eq!(pairing.discord_user_id, "discord1");
    }

    #[test]
    fn test_get_nonexistent_pairing() {
        let (db, _dir) = test_db();
        let result = db.get_pairing_by_discord("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_pending_pairing() {
        let (db, _dir) = test_db();
        db.create_pending_pairing("ABCD-1234", "discord1", "2025-01-01")
            .unwrap();
        let pending = db.get_pending_pairing("ABCD-1234").unwrap().unwrap();
        assert_eq!(pending.discord_user_id, "discord1");
        db.delete_pending_pairing("ABCD-1234").unwrap();
        assert!(db.get_pending_pairing("ABCD-1234").unwrap().is_none());
    }
}
