use std::sync::OnceLock;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum GatewayEvent {
    FileUpload {
        user_id: String,
        file_path: String,
        file_name: String,
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
        channel_id: String,
        content: String,
    },
    VoiceTts {
        guild_id: u64,
        audio_path: String,
    },
}

static EVENT_TX: OnceLock<broadcast::Sender<GatewayEvent>> = OnceLock::new();

pub fn init() -> broadcast::Sender<GatewayEvent> {
    let (tx, _) = broadcast::channel(100);
    let _ = EVENT_TX.set(tx.clone());
    tx
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

pub fn broadcast_file_upload(user_id: &str, file_path: &str, file_name: &str) {
    broadcast_event(GatewayEvent::FileUpload {
        user_id: user_id.to_string(),
        file_path: file_path.to_string(),
        file_name: file_name.to_string(),
    });
}

pub fn broadcast_channel_message(channel_id: &str, content: &str) {
    broadcast_event(GatewayEvent::ChannelMessage {
        channel_id: channel_id.to_string(),
        content: content.to_string(),
    });
}

pub fn broadcast_voice_tts(guild_id: u64, audio_path: &str) {
    broadcast_event(GatewayEvent::VoiceTts {
        guild_id,
        audio_path: audio_path.to_string(),
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
}
