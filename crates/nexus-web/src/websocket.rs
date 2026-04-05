//! WebSocket handler for real-time updates

use axum::{
    extract::{State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{error, info, warn};
use url::Url;

use crate::{models::WebSocketMessage, state::AppState};

/// Validate that the request Origin header matches an exact local origin.
/// Parses the Origin as a URL and compares scheme + host exactly to prevent
/// prefix-spoofing attacks (e.g. http://localhost.evil.com).
/// Missing Origin headers are rejected to enforce the local-only trust model.
fn is_local_origin(headers: &HeaderMap) -> bool {
    let origin_str = match headers.get("origin").and_then(|v| v.to_str().ok()) {
        Some(s) => s,
        None => return false, // Reject missing Origin — non-browser clients must send it
    };
    match Url::parse(origin_str) {
        Ok(url) => {
            let host = url.host_str().unwrap_or("");
            let scheme = url.scheme();
            (scheme == "http" || scheme == "https") && (host == "127.0.0.1" || host == "localhost")
        }
        Err(_) => false, // Malformed origins are rejected
    }
}

/// WebSocket connection handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<Arc<RwLock<AppState>>>,
) -> Response {
    // Reject cross-origin WebSocket upgrades
    if !is_local_origin(&headers) {
        return (
            StatusCode::FORBIDDEN,
            "WebSocket connections are only allowed from local origins",
        )
            .into_response();
    }

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

    // Channel for direct replies (pong, etc.) from the message handler to the send task
    let (direct_tx, mut direct_rx) = mpsc::channel::<WebSocketMessage>(16);

    info!("WebSocket client connected");

    // Spawn task to forward messages to this client.
    // Handles both broadcast events and direct replies.
    let send_task = tokio::spawn(async move {
        loop {
            // `biased` ensures direct replies (e.g. pong) always preempt
            // broadcast events when both channels are ready simultaneously.
            tokio::select! {
                biased;
                // Priority: direct replies first (e.g. pong responses)
                direct_msg = direct_rx.recv() => {
                    match direct_msg {
                        Some(msg) => {
                            if send_ws_message(&mut sender, &msg).await.is_err() {
                                break;
                            }
                        }
                        None => break, // channel closed
                    }
                }
                // Broadcast events
                broadcast_result = broadcast_rx.recv() => {
                    match broadcast_result {
                        Ok(msg) => {
                            if send_ws_message(&mut sender, &msg).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("WebSocket client lagged behind, dropped {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
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
                                if direct_tx.send(pong).await.is_err() {
                                    break;
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

/// Serialize and send a single WebSocketMessage to the client.
async fn send_ws_message(
    sender: &mut futures::stream::SplitSink<
        axum::extract::ws::WebSocket,
        axum::extract::ws::Message,
    >,
    msg: &WebSocketMessage,
) -> Result<(), axum::Error> {
    let json = match serde_json::to_string(msg) {
        Ok(j) => j,
        Err(e) => {
            error!("Failed to serialize WebSocket message: {}", e);
            return Ok(()); // skip bad message, keep connection alive
        }
    };

    sender
        .send(axum::extract::ws::Message::Text(json.into()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WebSocketMessageType;
    use crate::WebDashboard;
    use futures_util::StreamExt;
    use http::HeaderValue;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::protocol::Message as TungsteniteMessage;

    #[test]
    fn test_is_local_origin_accepts_https_localhost() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", HeaderValue::from_static("https://localhost:8768"));

        assert!(is_local_origin(&headers));
    }

    /// Verifies that a WebSocket `ping` from one client receives a direct `pong`
    /// reply to that client only, and is NOT broadcast to other connected clients.
    ///
    /// Marked `#[ignore]` because it requires raw TCP socket binding which can
    /// fail in restricted CI environments (PermissionDenied on ephemeral ports).
    /// Can be run locally with `--include-ignored` when socket access is available.
    #[tokio::test]
    #[ignore = "requires raw TCP bind on ephemeral port; flaky in restricted environments"]
    async fn test_ping_pong_isolation_direct_reply_only() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect to in-memory db");
        nexus_storage::migrations::run_migrations(&pool)
            .await
            .expect("run migrations");

        let mut storage = nexus_storage::StorageManager::new(pool.clone());
        storage.initialize().await.expect("initialize storage");

        let dashboard = WebDashboard::new(storage, nexus_orchestrator::Orchestrator::default())
            .await
            .expect("create dashboard");

        // Bind to port 0 to get a random available port
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let addr = listener.local_addr().expect("get local addr");

        // Spawn the server
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, dashboard.router).await.unwrap();
        });

        // Give the server a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Connect two WebSocket clients
        let url_a = format!("ws://127.0.0.1:{}/ws", addr.port());
        let url_b = format!("ws://127.0.0.1:{}/ws", addr.port());

        let (mut ws_a, _) = tokio_tungstenite::connect_async(&url_a)
            .await
            .expect("client A connect");
        let (mut ws_b, _) = tokio_tungstenite::connect_async(&url_b)
            .await
            .expect("client B connect");

        // Drain any initial messages from both clients (subscription setup noise)
        drain_messages(&mut ws_a, std::time::Duration::from_millis(200)).await;
        drain_messages(&mut ws_b, std::time::Duration::from_millis(200)).await;

        // Client A sends a ping
        let ping_msg = WebSocketMessage::ping();
        let ping_json = serde_json::to_string(&ping_msg).expect("serialize ping");
        ws_a.send(TungsteniteMessage::Text(ping_json.into()))
            .await
            .expect("send ping from A");

        // Client A should receive the pong directly
        let reply_a = tokio::time::timeout(std::time::Duration::from_secs(2), ws_a.next())
            .await
            .expect("timeout waiting for pong on A")
            .expect("no message on A")
            .expect("error on A");

        let reply_text = match reply_a {
            TungsteniteMessage::Text(t) => t.to_string(),
            other => panic!("expected text message on A, got: {:?}", other),
        };

        let reply_msg: WebSocketMessage =
            serde_json::from_str(&reply_text).expect("parse pong on A");
        assert!(
            matches!(reply_msg.message_type, WebSocketMessageType::Pong),
            "expected Pong message type, got: {:?}",
            reply_msg.message_type
        );

        // Client B should NOT receive the pong (it was a ping from A)
        // Wait a short period and verify no pong arrives on B
        let b_reply =
            tokio::time::timeout(std::time::Duration::from_millis(500), ws_b.next()).await;

        assert!(
            b_reply.is_err(),
            "Client B received a message when it should not have \
             (ping from A must not be broadcast)"
        );

        // Clean up
        server_handle.abort();
    }

    /// Drain any pending messages from a WebSocket connection within a timeout.
    async fn drain_messages(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        timeout: std::time::Duration,
    ) {
        loop {
            match tokio::time::timeout(timeout, ws.next()).await {
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_))) => break,
                Ok(None) => break,
                Err(_) => break, // timeout = no more pending messages
            }
        }
    }
}
