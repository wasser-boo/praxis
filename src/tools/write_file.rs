pub async fn write_file(path: &str, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_write_file() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();
        write_file(path, "hello world").await.unwrap();
        let content = std::fs::read_to_string(path).unwrap();
        assert_eq!(content, "hello world");
    }
}
