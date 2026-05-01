use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub content: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub updated_at: Option<String>,
}

impl Database {
    pub fn get_template(&self, name: &str) -> anyhow::Result<Option<Template>> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT name, content, description, is_system, updated_at FROM templates WHERE name = ?1",
            rusqlite::params![name],
            |row| {
                Ok(Template {
                    name: row.get(0)?,
                    content: row.get(1)?,
                    description: row.get(2)?,
                    is_system: row.get::<_, i32>(3)? != 0,
                    updated_at: row.get(4)?,
                })
            },
        );

        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_template(
        &self,
        name: &str,
        content: &str,
        description: Option<&str>,
        is_system: bool,
    ) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR REPLACE INTO templates (name, content, description, is_system, updated_at) VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            rusqlite::params![name, content, description, is_system as i32],
        )?;
        Ok(())
    }

    pub fn list_templates(&self) -> anyhow::Result<Vec<Template>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT name, content, description, is_system, updated_at FROM templates ORDER BY name",
        )?;

        let templates = stmt
            .query_map([], |row| {
                Ok(Template {
                    name: row.get(0)?,
                    content: row.get(1)?,
                    description: row.get(2)?,
                    is_system: row.get::<_, i32>(3)? != 0,
                    updated_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(templates)
    }

    pub fn delete_template(&self, name: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM templates WHERE name = ?1",
            rusqlite::params![name],
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

    #[test]
    fn test_save_and_get_template() {
        let (db, _dir) = test_db();
        db.save_template("test", "Hello {{name}}", Some("Test template"), false)
            .unwrap();
        let tmpl = db.get_template("test").unwrap().unwrap();
        assert_eq!(tmpl.content, "Hello {{name}}");
        assert_eq!(tmpl.description, Some("Test template".to_string()));
        assert!(!tmpl.is_system);
    }

    #[test]
    fn test_list_templates() {
        let (db, _dir) = test_db();
        db.save_template("a", "content a", None, false).unwrap();
        db.save_template("b", "content b", None, true).unwrap();
        let templates = db.list_templates().unwrap();
        assert_eq!(templates.len(), 2);
    }

    #[test]
    fn test_delete_template() {
        let (db, _dir) = test_db();
        db.save_template("test", "content", None, false).unwrap();
        db.delete_template("test").unwrap();
        assert!(db.get_template("test").unwrap().is_none());
    }
}
