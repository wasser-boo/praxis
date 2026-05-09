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
