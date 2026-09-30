//! Bounded, session-scoped response snapshots. No paths supplied by the model.
use super::{contexts::Context, Database};
use rusqlite::{params, Connection, OptionalExtension};

pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_USER_OUTPUTS: usize = 200;
pub const MAX_USER_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_GLOBAL_OUTPUTS: usize = 2000;
pub const MAX_GLOBAL_BYTES: usize = 512 * 1024 * 1024;
pub const RETENTION_SECONDS: i64 = 7 * 24 * 3600;

pub struct ToolOutput {
    pub id: String,
    pub tool_name: String,
    pub call_id: String,
    pub content: String,
    pub source_bytes: usize,
    pub source_chars: usize,
}

pub(super) fn active_session(conn: &Connection, user: &str) -> anyhow::Result<String> {
    let data: Option<String> = conn.query_row("SELECT data FROM contexts WHERE user_id=?1", [user], |r| r.get(0)).optional()?;
    let ctx: Context = match data { Some(data) => serde_json::from_str(&data)?, None => Context::default() };
    Ok(normalize_session(&ctx.session_id).to_string())
}
pub(super) fn normalize_session(session: &str) -> &str {
    if session == "default" { "" } else { session }
}
pub(super) fn clear_session(conn: &Connection, user: &str, session: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM tool_outputs WHERE owner_id=?1 AND session_id=?2", params![user, normalize_session(session)])?;
    Ok(())
}
fn prune(conn: &Connection, now: i64) -> anyhow::Result<()> {
    conn.execute("DELETE FROM tool_outputs WHERE created_at < ?1", [now - RETENTION_SECONDS])?;
    conn.execute("DELETE FROM tool_outputs WHERE seq IN (
        SELECT seq FROM (SELECT seq, SUM(retained_bytes) OVER (PARTITION BY owner_id ORDER BY seq DESC) AS bytes,
        ROW_NUMBER() OVER (PARTITION BY owner_id ORDER BY seq DESC) AS n FROM tool_outputs)
        WHERE bytes > ?1 OR n > ?2)", params![MAX_USER_BYTES, MAX_USER_OUTPUTS])?;
    conn.execute("DELETE FROM tool_outputs WHERE seq IN (
        SELECT seq FROM (SELECT seq, SUM(retained_bytes) OVER (ORDER BY seq DESC) AS bytes,
        ROW_NUMBER() OVER (ORDER BY seq DESC) AS n FROM tool_outputs)
        WHERE bytes > ?1 OR n > ?2)", params![MAX_GLOBAL_BYTES, MAX_GLOBAL_OUTPUTS])?;
    Ok(())
}
impl Database {
    pub fn save_tool_output(&self, user: &str, tool: &str, call: &str, text: &str) -> anyhow::Result<ToolOutput> {
        let mut end = text.len().min(MAX_OUTPUT_BYTES);
        while !text.is_char_boundary(end) { end -= 1; }
        let output = ToolOutput {
            id: format!("out_{}", uuid::Uuid::new_v4().simple()),
            tool_name: tool.chars().take(256).collect(),
            call_id: call.chars().take(256).collect(),
            content: text[..end].to_string(),
            source_bytes: text.len(),
            source_chars: text.chars().count(),
        };
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let session = active_session(&tx, user)?;
        let now = chrono::Utc::now().timestamp();
        tx.execute("INSERT INTO tool_outputs(id,owner_id,session_id,tool_name,call_id,content,retained_bytes,source_bytes,source_chars,created_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![output.id, user, session, output.tool_name,
                output.call_id, output.content, output.content.len(), output.source_bytes, output.source_chars, now])?;
        prune(&tx, now)?;
        tx.commit()?;
        Ok(output)
    }
    pub fn get_tool_output(&self, user: &str, id: &str) -> anyhow::Result<Option<ToolOutput>> {
        let conn = self.conn();
        let session = active_session(&conn, user)?;
        Ok(conn.query_row("SELECT id,tool_name,call_id,content,source_bytes,source_chars FROM tool_outputs
            WHERE id=?1 AND owner_id=?2 AND session_id=?3 AND created_at>=?4",
            params![id, user, session, chrono::Utc::now().timestamp() - RETENTION_SECONDS], |r| Ok(ToolOutput {
                id: r.get(0)?, tool_name: r.get(1)?, call_id: r.get(2)?, content: r.get(3)?, source_bytes: r.get(4)?, source_chars: r.get(5)?,
            })).optional()?)
    }
    pub fn prune_tool_outputs(&self) -> anyhow::Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        prune(&tx, chrono::Utc::now().timestamp())?;
        tx.commit()?;
        Ok(())
    }
}
