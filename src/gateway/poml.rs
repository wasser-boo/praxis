use std::process::Command;
use tempfile::NamedTempFile;

pub async fn render(template_path: &str, context: &serde_json::Value) -> anyhow::Result<String> {
    let poml_cli = std::env::var("POML_CLI").unwrap_or_else(|_| "poml".to_string());

    let context_file = NamedTempFile::new()?;
    std::fs::write(
        context_file.path(),
        serde_json::to_string_pretty(context)?,
    )?;

    let output = Command::new(&poml_cli)
        .arg("render")
        .arg(template_path)
        .arg("--context")
        .arg(context_file.path())
        .arg("--format")
        .arg("text")
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let result = String::from_utf8(output.stdout)?;
            Ok(strip_think_tags(&result))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("POML CLI error: {}", stderr);
            render_simple(template_path, context).await
        }
        Err(e) => {
            tracing::warn!("POML CLI not found: {}, using simple template", e);
            render_simple(template_path, context).await
        }
    }
}

async fn render_simple(
    template_path: &str,
    context: &serde_json::Value,
) -> anyhow::Result<String> {
    let template = std::fs::read_to_string(template_path)?;

    let mut result = template;
    if let Some(obj) = context.as_object() {
        for (key, value) in obj {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }

    Ok(result)
}

pub fn strip_think_tags(content: &str) -> String {
    let re = regex::Regex::new(r"[\s]*<think>.*?</think>[\s]*").unwrap();
    let result = re.replace_all(content, " ");
    result.trim().to_string()
}

pub fn extract_agent_signals(content: &str) -> (String, Vec<String>) {
    let mut signals = Vec::new();
    let mut clean = content.to_string();

    if content.contains("[[AGENT:NEXT]]") {
        signals.push("next".to_string());
        clean = clean.replace("[[AGENT:NEXT]]", "");
    }

    if content.contains("[[AGENT:COMPLETE]]") {
        signals.push("complete".to_string());
        clean = clean.replace("[[AGENT:COMPLETE]]", "");
    }

    (clean.trim().to_string(), signals)
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_strip_think_tags() {
        let input = "Hello <think>thinking here</think> World";
        let result = strip_think_tags(input);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_extract_agent_signals_next() {
        let (clean, signals) = extract_agent_signals("Done [[AGENT:NEXT]]");
        assert_eq!(clean, "Done");
        assert!(signals.contains(&"next".to_string()));
    }

    #[test]
    fn test_extract_agent_signals_complete() {
        let (clean, signals) = extract_agent_signals("Finished [[AGENT:COMPLETE]]");
        assert_eq!(clean, "Finished");
        assert!(signals.contains(&"complete".to_string()));
    }

    #[test]
    fn test_extract_no_signals() {
        let (clean, signals) = extract_agent_signals("Just text");
        assert_eq!(clean, "Just text");
        assert!(signals.is_empty());
    }
}
