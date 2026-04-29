pub fn get_context(user_id: &str, key: &str) -> anyhow::Result<Option<String>> {
    tracing::info!("Getting context for user {} key {}", user_id, key);
    Ok(None)
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn test_get_context() {
        let result = get_context("user1", "mode").unwrap();
        assert!(result.is_none());
    }
}
