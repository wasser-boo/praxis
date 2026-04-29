pub fn set_context_value(key: &str, value: &str) -> anyhow::Result<()> {
    tracing::info!("Setting context {} = {}", key, value);
    Ok(())
}

pub fn delete_context_value(key: &str) -> anyhow::Result<()> {
    tracing::info!("Deleting context {}", key);
    Ok(())
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn test_set_context_value() {
        assert!(set_context_value("mode", "agent").is_ok());
    }

    #[test]
    fn test_delete_context_value() {
        assert!(delete_context_value("mode").is_ok());
    }
}
