//! WebSocket handler for real-time updates

use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::{models::WebSocketMessage, state::AppState};

/// WebSocket connection handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<RwLock<AppState>>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle a WebSocket connection
async fn handle_socket(socket: axum::extract::ws::WebSocket, state: Arc<RwLock<AppState>>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel
    let mut broadcast_rx = {
        let state = state.read().await;
        state.subscribe_ws()
    };

    info!("WebSocket client connected");

    // Spawn task to forward broadcast messages to this client
    let send_task = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    let json = match serde_json::to_string(&msg) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize WebSocket message: {}", e);
                            continue;
                        }
                    };

                    if sender
                        .send(axum::extract::ws::Message::Text(json.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    error!("Broadcast receive error: {}", e);
                    break;
                }
            }
        }
    });

    // Handle incoming messages from client
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                // Parse the message
                match serde_json::from_str::<WebSocketMessage>(&text) {
                    Ok(ws_msg) => {
                        // Handle ping/pong
                        match ws_msg.message_type {
                            crate::models::WebSocketMessageType::Ping => {
                                let pong = WebSocketMessage::pong();
                                let app_state = state.read().await;
                                if let Err(e) = app_state.broadcast_ws(pong) {
                                    warn!("Failed to broadcast WebSocket pong: {}", e);
                                }
                            }
                            _ => {
                                // Handle other message types if needed
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Invalid WebSocket message received: {}", e);
                    }
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => {
                info!("WebSocket client disconnected");
                break;
            }
            Ok(_) => {
                // Ignore other message types
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    // Abort the send task when client disconnects
    send_task.abort();
    info!("WebSocket connection closed");
}
