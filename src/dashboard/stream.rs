use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize)]
pub struct StreamEvent {
    pub event: String,
    pub data: String,
}

static USER_STREAMS: once_cell::sync::Lazy<Arc<DashMap<String, broadcast::Sender<StreamEvent>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(DashMap::new()));

pub fn get_or_create(user_id: &str) -> broadcast::Sender<StreamEvent> {
    let existed = USER_STREAMS.contains_key(user_id);
    let tx = USER_STREAMS
        .entry(user_id.to_string())
        .or_insert_with(|| {
            tracing::info!(user_id = %user_id, num_receivers = 0, "[STREAM] channel created");
            broadcast::channel(8192).0
        })
        .clone();
    if !existed {
        tracing::info!(user_id = %user_id, "[STREAM] channel created");
    }
    tx
}

pub fn subscribe(user_id: &str) -> Option<broadcast::Receiver<StreamEvent>> {
    let rx = USER_STREAMS.get(user_id).map(|s| {
        let r = s.value().subscribe();
        tracing::info!(user_id = %user_id, "[STREAM] new subscriber");
        r
    });
    if rx.is_none() {
        tracing::warn!(user_id = %user_id, "[STREAM] subscribe failed: no channel for user");
    }
    rx
}

/// Return the number of current subscribers (receivers) for a user.
pub fn subscriber_count(user_id: &str) -> usize {
    USER_STREAMS
        .get(user_id)
        .map(|s| s.value().receiver_count())
        .unwrap_or(0)
}

/// Return whether at least one subscriber is currently connected.
pub fn has_subscriber(user_id: &str) -> bool {
    subscriber_count(user_id) > 0
}

/// Wait until at least one subscriber is connected, or timeout.
/// Useful to gate LLM streaming until the browser SSE is open.
pub async fn wait_for_subscriber(user_id: &str, timeout_ms: u64) {
    let mut waited_ms = 0u64;
    loop {
        let count = subscriber_count(user_id);
        if count > 0 {
            tracing::info!(user_id = %user_id, waited_ms = waited_ms, count = count, "[STREAM] subscriber ready");
            return;
        }
        if waited_ms >= timeout_ms {
            tracing::warn!(user_id = %user_id, waited_ms = waited_ms, "[STREAM] timeout waiting for subscriber");
            return;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        waited_ms += 100;
    }
}

pub fn remove(user_id: &str) {
    USER_STREAMS.remove(user_id);
    tracing::info!(user_id = %user_id, "[STREAM] channel removed");
}

pub fn send(user_id: &str, event: &str, data: &str) {
    let data_preview = if data.len() > 80 {
        format!("{}...", &data[..80])
    } else {
        data.to_string()
    };
    let tx = get_or_create(user_id);
    match tx.send(StreamEvent {
        event: event.to_string(),
        data: data.to_string(),
    }) {
        Ok(num_receivers) => {
            tracing::debug!(
                user_id = %user_id,
                event = %event,
                data_preview = %data_preview,
                num_receivers = num_receivers,
                "[STREAM] event sent"
            );
        }
        Err(_) => {
            tracing::warn!(
                user_id = %user_id,
                event = %event,
                data_preview = %data_preview,
                "[STREAM] send failed: no receivers"
            );
        }
    }
}
