use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use crate::gateway::GatewayState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<GatewayState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, _state: GatewayState) {
    let (mut sender, mut receiver) = socket.split();

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                tracing::warn!("WebSocket error: {}", e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                tracing::debug!("Received text: {}", text);
                let response = serde_json::json!({
                    "type": "echo",
                    "data": text
                });
                if sender
                    .send(Message::Text(response.to_string()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Message::Binary(_) => {
                tracing::debug!("Received binary data");
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_ws_handler_compiles() {
        assert!(true);
    }
}
