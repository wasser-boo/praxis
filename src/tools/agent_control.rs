use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentSignal {
    Continue,
    Stop,
    Feedback(String),
}

pub fn process_signal(signal: &str) -> AgentSignal {
    match signal {
        "continue" => AgentSignal::Continue,
        "stop" => AgentSignal::Stop,
        msg => AgentSignal::Feedback(msg.to_string()),
    }
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[test]
    fn test_process_signal_continue() {
        match process_signal("continue") {
            AgentSignal::Continue => {}
            _ => panic!("Wrong signal"),
        }
    }

    #[test]
    fn test_process_signal_stop() {
        match process_signal("stop") {
            AgentSignal::Stop => {}
            _ => panic!("Wrong signal"),
        }
    }

    #[test]
    fn test_process_signal_feedback() {
        match process_signal("good job") {
            AgentSignal::Feedback(msg) => assert_eq!(msg, "good job"),
            _ => panic!("Wrong signal"),
        }
    }
}
