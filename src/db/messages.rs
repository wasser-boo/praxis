use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub id: String,
    pub function: FunctionCallData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallData {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallData>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_parts: Option<Vec<serde_json::Value>>,
}

impl Message {
    pub fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content,
            tool_call_id: None,
            tool_calls: None,
            content_parts: None,
        }
    }

    pub fn assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls: None,
            content_parts: None,
        }
    }

    pub fn assistant_with_tool_calls(content: String, tool_calls: Vec<ToolCallData>) -> Self {
        Self {
            role: "assistant".to_string(),
            content,
            tool_call_id: None,
            tool_calls: Some(tool_calls),
            content_parts: None,
        }
    }

    pub fn tool(content: String, tool_call_id: String) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_call_id: Some(tool_call_id),
            tool_calls: None,
            content_parts: None,
        }
    }

    pub fn tool_with_image(
        content: String,
        tool_call_id: String,
        content_parts: Vec<serde_json::Value>,
    ) -> Self {
        Self {
            role: "tool".to_string(),
            content,
            tool_call_id: Some(tool_call_id),
            tool_calls: None,
            content_parts: Some(content_parts),
        }
    }
}

pub fn estimate_message_tokens(msg: &Message) -> usize {
    let mut tokens = 4usize;
    tokens += msg.role.len() / 4 + 1;
    tokens += msg.content.len() / 4 + 1;
    if let Some(ref tcid) = msg.tool_call_id {
        tokens += tcid.len() / 4 + 1;
    }
    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            tokens += 4;
            tokens += tc.id.len() / 4 + 1;
            tokens += tc.function.name.len() / 4 + 1;
            tokens += tc.function.arguments.len() / 4 + 1;
        }
    }
    tokens
}

impl Database {
    pub fn add_message(&self, user_id: &str, msg: &Message) -> anyhow::Result<i64> {
        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .map(|tc| serde_json::to_string(tc).unwrap_or_default());
        let conn = self.conn();
        let id = conn.execute(
            "INSERT INTO messages (user_id, role, content, tool_call_id, tool_calls) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![user_id, msg.role, msg.content, msg.tool_call_id, tool_calls_json],
        )?;
        Ok(id as i64)
    }

    pub fn get_messages(&self, user_id: &str, limit: i32) -> anyhow::Result<Vec<Message>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls FROM messages WHERE user_id = ?1 ORDER BY id DESC LIMIT ?2"
        )?;

        let messages = stmt
            .query_map(rusqlite::params![user_id, limit], |row| {
                let tool_calls_json: Option<String> = row.get(3)?;
                let tool_calls: Option<Vec<ToolCallData>> =
                    tool_calls_json.and_then(|json| serde_json::from_str(&json).ok());
                Ok(Message {
                    role: row.get(0)?,
                    content: row.get(1)?,
                    tool_call_id: row.get(2)?,
                    tool_calls,
                    content_parts: None,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(messages.into_iter().rev().collect())
    }

    pub fn get_messages_with_token_budget(
        &self,
        user_id: &str,
        token_budget: usize,
    ) -> anyhow::Result<(Vec<Message>, usize)> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT role, content, tool_call_id, tool_calls FROM messages WHERE user_id = ?1 ORDER BY id DESC",
        )?;

        let mut messages = Vec::new();
        let mut total_tokens = 0usize;
        let rows = stmt.query_map(rusqlite::params![user_id], |row| {
            let tool_calls_json: Option<String> = row.get(3)?;
            let tool_calls: Option<Vec<ToolCallData>> =
                tool_calls_json.and_then(|json| serde_json::from_str(&json).ok());
            Ok(Message {
                role: row.get(0)?,
                content: row.get(1)?,
                tool_call_id: row.get(2)?,
                tool_calls,
                content_parts: None,
            })
        })?;

        for row in rows {
            let msg = row?;
            let tokens = estimate_message_tokens(&msg);
            if total_tokens + tokens > token_budget && !messages.is_empty() {
                break;
            }
            total_tokens += tokens;
            messages.push(msg);
        }

        messages.reverse();
        Ok((messages, total_tokens))
    }

    pub fn count_messages(&self, user_id: &str) -> anyhow::Result<usize> {
        let conn = self.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE user_id = ?1",
            rusqlite::params![user_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
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
        let msg = Message::user("Hello".to_string());
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
            let msg = Message::user(format!("Message {}", i));
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
        let msg = Message::user("Hello".to_string());
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
            let msg = Message::user(format!("msg{}", i));
            db.add_message("user1", &msg).unwrap();
        }
        let messages = db.get_messages("user1", 10).unwrap();
        assert_eq!(messages[0].content, "msg0");
        assert_eq!(messages[2].content, "msg2");
    }

    #[test]
    fn test_message_with_tool_calls() {
        let (db, _dir) = test_db();
        ensure_context(&db, "user1");
        let msg = Message::assistant_with_tool_calls(
            String::new(),
            vec![ToolCallData {
                id: "call_123".to_string(),
                function: FunctionCallData {
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"/tmp/test"}"#.to_string(),
                },
            }],
        );
        db.add_message("user1", &msg).unwrap();

        let messages = db.get_messages("user1", 10).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "assistant");
        let tool_calls = messages[0].tool_calls.as_ref().unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_123");
        assert_eq!(tool_calls[0].function.name, "read_file");
    }
}
