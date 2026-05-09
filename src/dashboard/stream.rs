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
    USER_STREAMS
        .entry(user_id.to_string())
        .or_insert_with(|| broadcast::channel(8192).0)
        .clone()
}

pub fn subscribe(user_id: &str) -> Option<broadcast::Receiver<StreamEvent>> {
    USER_STREAMS.get(user_id).map(|s| s.value().subscribe())
}

pub fn remove(user_id: &str) {
    USER_STREAMS.remove(user_id);
}

pub fn send(user_id: &str, event: &str, data: &str) {
    let tx = get_or_create(user_id);
    let _ = tx.send(StreamEvent {
        event: event.to_string(),
        data: data.to_string(),
    });
}
