use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id: i64,
    pub document_id: String,
    pub chunk_index: usize,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub token_count: Option<usize>,
}

pub async fn rag_query(
    db: &crate::db::Database,
    user_id: &str,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<DocumentChunk>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT id, document_id, chunk_index, content, metadata, token_count 
         FROM document_chunks 
         WHERE user_id = ?1 AND content LIKE ?2 
         ORDER BY chunk_index 
         LIMIT ?3",
    )?;

    let query_pattern = format!("%{}%", query);
    let chunks = stmt
        .query_map(
            rusqlite::params![user_id, query_pattern, limit as i64],
            |row| {
                Ok(DocumentChunk {
                    id: row.get(0)?,
                    document_id: row.get(1)?,
                    chunk_index: row.get::<_, i64>(2)? as usize,
                    content: row.get(3)?,
                    metadata: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    token_count: row.get::<_, Option<i64>>(5)?.map(|v| v as usize),
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(chunks)
}

pub fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < words.len() {
        let end = (start + chunk_size).min(words.len());
        let chunk = words[start..end].join(" ");
        chunks.push(chunk);

        if end >= words.len() {
            break;
        }
        start += chunk_size - overlap;
    }

    chunks
}

#[cfg(test)]
mod rag_tests {
    use super::*;

    #[test]
    fn test_chunk_text_basic() {
        let text = "one two three four five six seven eight nine ten";
        let chunks = chunk_text(text, 3, 0);
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0], "one two three");
        assert_eq!(chunks[1], "four five six");
    }

    #[test]
    fn test_chunk_text_with_overlap() {
        let text = "one two three four five";
        let chunks = chunk_text(text, 3, 1);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "one two three");
        assert_eq!(chunks[1], "three four five");
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 100, 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_short() {
        let text = "hello world";
        let chunks = chunk_text(text, 100, 0);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "hello world");
    }
}
