//! Keep WebSocket feedback and control input alive during a long LLM call.
use super::{Message, WebSocket, WsIncoming, WsOutgoing};
use crate::gateway::{task_control, GatewayState};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};

pub(super) async fn handle(
    state: &GatewayState,
    user: &str,
    content: &str,
    channel: Option<&str>,
    sender: &mut SplitSink<WebSocket, Message>,
    receiver: &mut SplitStream<WebSocket>,
) -> anyhow::Result<String> {
    let _owner = task_control::begin(user)?;
    let mut feedback = crate::dashboard::stream::get_or_create(user).subscribe();
    let work = crate::gateway::message_handler::handle_message_inner(state, user, content, channel);
    tokio::pin!(work);
    let mut connected = true;
    loop {
        tokio::select! {
            result = &mut work => return result,
            event = feedback.recv(), if connected => {
                if let Ok(event) = event {
                    if event.event == "feedback" {
                        let message = WsOutgoing::Feedback { user_id: user.into(), content: event.data };
                        if sender.send(Message::Text(serde_json::to_string(&message)?)).await.is_err() {
                            connected = false;
                            task_control::cancel(user);
                        }
                    }
                }
            }
            frame = receiver.next(), if connected => {
                match frame {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {
                        connected = false;
                        task_control::cancel(user);
                        // Do not drop `work`: a tool may already have made a
                        // side effect. Let it finish/persist, then cancellation
                        // prevents further tools and interrupts LLM waits.
                    }
                    Some(Ok(Message::Text(text))) => {
                        let reply = match serde_json::from_str::<WsIncoming>(&text) {
                            Ok(WsIncoming::Ping) => WsOutgoing::Pong,
                            Ok(WsIncoming::Stop { user_id }) if user_id == user => {
                                task_control::cancel(user);
                                WsOutgoing::Feedback { user_id, content: "Stopping; any already-running tool will finish saving its result.".into() }
                            }
                            Ok(WsIncoming::AgentInput { user_id, content }) if user_id == user => {
                                let accepted = if let Some(tx) = crate::gateway::agent_loop::get_user_input_sender(user).await { tx.send(content).is_ok() } else { false };
                                WsOutgoing::Feedback { user_id, content: if accepted { "Input queued for the next agent turn." } else { "No agent input queue; wait for this request or stop it first." }.into() }
                            }
                            _ => WsOutgoing::Feedback { user_id: user.into(), content: "Request not accepted: this connection has an active task. Use agent_input or stop, or wait for its final response.".into() },
                        };
                        if sender.send(Message::Text(serde_json::to_string(&reply)?)).await.is_err() {
                            connected = false;
                            task_control::cancel(user);
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() { connected = false; task_control::cancel(user); }
                    }
                    _ => {}
                }
            }
        }
    }
}
