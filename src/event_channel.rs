use std::sync::OnceLock;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    FileUpload {
        user_id: String,
        file_path: String,
        file_name: String,
        channel_id: String,
    },
    AgentComplete {
        user_id: String,
        response: String,
    },
    AgentFeedback {
        user_id: String,
        message: String,
    },
    ChannelMessage {
        user_id: String,
        channel_id: String,
        message: String,
    },
    VoiceTts {
        user_id: String,
        audio_data: Vec<u8>,
    },
}

static EVENT_TX: OnceLock<broadcast::Sender<GatewayEvent>> = OnceLock::new();

pub fn init() -> broadcast::Sender<GatewayEvent> {
    let (tx, _) = broadcast::channel(100);
    let _ = EVENT_TX.set(tx.clone());
    tx
}

pub fn get_event_tx() -> Option<broadcast::Sender<GatewayEvent>> {
    EVENT_TX.get().cloned()
}

pub fn sender() -> broadcast::Sender<GatewayEvent> {
    EVENT_TX.get().unwrap().clone()
}

pub fn broadcast_event(event: GatewayEvent) {
    if let Some(tx) = EVENT_TX.get() {
        let _ = tx.send(event);
    }
}

pub fn broadcast_agent_complete(user_id: &str, response: &str) {
    broadcast_event(GatewayEvent::AgentComplete {
        user_id: user_id.to_string(),
        response: response.to_string(),
    });
}

pub fn broadcast_agent_feedback(user_id: &str, message: &str) {
    broadcast_event(GatewayEvent::AgentFeedback {
        user_id: user_id.to_string(),
        message: message.to_string(),
    });
}

pub fn broadcast_file_upload(user_id: &str, file_path: &str, file_name: &str, channel_id: &str) {
    broadcast_event(GatewayEvent::FileUpload {
        user_id: user_id.to_string(),
        file_path: file_path.to_string(),
        file_name: file_name.to_string(),
        channel_id: channel_id.to_string(),
    });
}

pub fn broadcast_channel_message(user_id: &str, channel_id: &str, message: &str) {
    broadcast_event(GatewayEvent::ChannelMessage {
        user_id: user_id.to_string(),
        channel_id: channel_id.to_string(),
        message: message.to_string(),
    });
}

pub fn broadcast_voice_tts(user_id: &str, audio_data: Vec<u8>) {
    broadcast_event(GatewayEvent::VoiceTts {
        user_id: user_id.to_string(),
        audio_data,
    });
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_event_bus_init() {
        let _tx = init();
        let _rx = sender().subscribe();
    }

    #[test]
    fn test_get_event_tx() {
        let _tx = init();
        assert!(get_event_tx().is_some());
    }

    #[test]
    fn test_broadcast_agent_complete() {
        let _tx = init();
        let mut rx = sender().subscribe();
        broadcast_agent_complete("user1", "hello");
        let event = rx.try_recv().unwrap();
        match event {
            GatewayEvent::AgentComplete { user_id, response } => {
                assert_eq!(user_id, "user1");
                assert_eq!(response, "hello");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_broadcast_agent_feedback() {
        let _tx = init();
        let mut rx = sender().subscribe();
        broadcast_agent_feedback("user1", "progress update");
        let event = rx.try_recv().unwrap();
        match event {
            GatewayEvent::AgentFeedback { user_id, message } => {
                assert_eq!(user_id, "user1");
                assert_eq!(message, "progress update");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_broadcast_file_upload() {
        let _tx = init();
        let mut rx = sender().subscribe();
        broadcast_file_upload("user1", "/tmp/file.txt", "file.txt", "ch123");
        let event = rx.try_recv().unwrap();
        match event {
            GatewayEvent::FileUpload { user_id, file_path, file_name, channel_id } => {
                assert_eq!(user_id, "user1");
                assert_eq!(file_path, "/tmp/file.txt");
                assert_eq!(file_name, "file.txt");
                assert_eq!(channel_id, "ch123");
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_broadcast_voice_tts() {
        let _tx = init();
        let mut rx = sender().subscribe();
        broadcast_voice_tts("user1", vec![1, 2, 3]);
        let event = rx.try_recv().unwrap();
        match event {
            GatewayEvent::VoiceTts { user_id, audio_data } => {
                assert_eq!(user_id, "user1");
                assert_eq!(audio_data, vec![1, 2, 3]);
            }
            _ => panic!("Wrong event type"),
        }
    }
}
