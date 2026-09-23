//! Live gateway subscription; persisted history remains authoritative.
use super::app::Bubble;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct LiveOutput {
    pub text: String,
    pub reasoning: String,
    pub tools: BTreeMap<usize, (String, String)>,
}
impl LiveOutput {
    pub fn apply(&mut self, event: &str, data: &str) {
        match event {
            "stream_start" | "stream_abort" => *self = Self::default(),
            "char" => self.text.push_str(data),
            "reasoning_delta" => self.reasoning.push_str(data),
            "reasoning" => self.reasoning = data.into(),
            "assistant_saved" => self.text.clear(),
            "tool_call_delta" => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(index) = value["index"].as_u64().filter(|n| *n < 128) {
                        let (name, args) = self.tools.entry(index as usize).or_default();
                        name.push_str(value["name"].as_str().unwrap_or(""));
                        args.push_str(value["arguments"].as_str().unwrap_or(""));
                    }
                }
            }
            _ => {}
        }
    }
    pub fn bubbles(&self) -> Vec<Bubble> {
        let mut out = Vec::new();
        if !self.reasoning.is_empty() { out.push(Bubble::Thinking {content:self.reasoning.clone()}); }
        if !self.text.is_empty() { out.push(Bubble::Assistant {content:self.text.clone()}); }
        for (name, args) in self.tools.values() {
            out.push(Bubble::Tool {name:format!("{name} — generating, not executed"),content:args.clone()});
        }
        out
    }
}

pub fn subscribe(base: String, key: String, user: String,
    tx: tokio::sync::mpsc::Sender<(String, String, String)>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(3)).build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let url = format!("{}/v1/events/{}", base.trim_end_matches('/'), urlencoding::encode(&user));
        loop {
            if tx.is_closed() { break; }
            if let Ok(response) = client.get(&url).bearer_auth(&key).send().await {
                if let Ok(mut response) = response.error_for_status() {
                    let mut decoder = crate::sse::Decoder::default();
                    while let Ok(Some(chunk)) = response.chunk().await {
                        let Ok(events) = decoder.push(&chunk) else { break };
                        for event in events {
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&event.data) {
                                if let Some(data) = value["data"].as_str() {
                                    if tx.send((user.clone(), event.event, data.into())).await.is_err() { return; }
                                }
                            }
                        }
                    }
                }
            }
            let _ = tx.send((user.clone(), "stream_abort".into(), "".into())).await;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tui_shows_tool_arguments_before_completion_without_executing() {
        let mut live = LiveOutput::default();
        live.apply("tool_call_delta", r#"{"index":0,"name":"read_file","arguments":"{\"pa"}"#);
        assert!(matches!(&live.bubbles()[0], Bubble::Tool {content,..} if content == "{\"pa"));
        live.apply("reasoning_delta", "thinking");
        live.apply("reasoning", "thinking");
        assert_eq!(live.reasoning, "thinking", "final event must not duplicate deltas");
        live.apply("stream_abort", "");
        assert!(live.bubbles().is_empty());
    }
}
