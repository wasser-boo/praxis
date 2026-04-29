use serde::{Deserialize, Serialize};
use super::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Database {
    pub fn add_message(&self, user_id: &str, msg: &Message) -> anyhow::Result<i64> {
        let conn = self.conn();
        let id = conn.execute(
            "INSERT INTO messages (user_id, role, content, tool_call_id) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![user_id, msg.role, msg.content, msg.tool_call_id],
        )?;
        Ok(id as i64)
    }

    pub fn get_messages(&self, user_id: &str, limit: i32) -> anyhow::Result<Vec<Message>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT role, content, tool_call_id FROM messages WHERE user_id = ?1 ORDER BY id DESC LIMIT ?2"
        )?;

        let messages = stmt
            .query_map(rusqlite::params![user_id, limit], |row| {
                Ok(Message {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    tool_call_id: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages.into_iter().rev().collect())
    }

    pub fn clear_messages(&self, user_id: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM messages WHERE user_id = ?1",
            rusqlite::params![user_id],
        )?;
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

    fn ensure_context(db: &Database, user_id: &str) {
        let ctx = crate::db::contexts::Context {
            user_id: user_id.to_string(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();
    }

    #[test]
    fn test_add_and_get_message() {
        let (db, _dir) = test_db();
        ensure_context(&db, "user1");
        let msg = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_call_id: None,
        };
        let id = db.add_message("user1", &msg).unwrap();
        assert!(id > 0);

        let messages = db.get_messages("user1", 10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Hello");
    }

    #[test]
    fn test_get_messages_with_limit() {
        let (db, _dir) = test_db();
        ensure_context(&db, "user1");
        for i in 0..5 {
            let msg = Message {
                role: "user".to_string(),
                content: format!("Message {}", i),
                tool_call_id: None,
            };
            db.add_message("user1", &msg).unwrap();
        }

        let messages = db.get_messages("user1", 3).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "Message 2");
    }

    #[test]
    fn test_clear_messages() {
        let (db, _dir) = test_db();
        ensure_context(&db, "user1");
        let msg = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_call_id: None,
        };
        db.add_message("user1", &msg).unwrap();
        db.clear_messages("user1").unwrap();
        let messages = db.get_messages("user1", 10).unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn test_message_order() {
        let (db, _dir) = test_db();
        ensure_context(&db, "user1");
        for i in 0..3 {
            let msg = Message {
                role: "user".to_string(),
                content: format!("msg{}", i),
                tool_call_id: None,
            };
            db.add_message("user1", &msg).unwrap();
        }
        let messages = db.get_messages("user1", 10).unwrap();
        assert_eq!(messages[0].content, "msg0");
        assert_eq!(messages[2].content, "msg2");
    }
}
