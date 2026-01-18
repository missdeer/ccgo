//! WebSocket handler for real-time PTY output

use super::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(agent_name): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, agent_name))
}

async fn handle_socket(socket: WebSocket, state: AppState, agent_name: String) {
    let (mut sender, mut receiver) = socket.split();

    // Get the PTY handle for this agent
    let Some(session) = state.session_manager.get(&agent_name).await else {
        let _ = sender
            .send(Message::Text(format!(
                "Error: Agent {} not found",
                agent_name
            )))
            .await;
        return;
    };

    let pty_guard = session.pty.read().await;
    let Some(pty) = pty_guard.as_ref() else {
        let _ = sender
            .send(Message::Text(format!(
                "Error: Agent {} not running",
                agent_name
            )))
            .await;
        return;
    };

    // Send buffered output first
    let buffer = pty.get_buffer().await;
    if !buffer.is_empty() {
        let _ = sender.send(Message::Binary(buffer)).await;
    }

    // Subscribe to new output
    let mut output_rx = pty.subscribe_output();
    drop(pty_guard);

    // Spawn task to forward PTY output to WebSocket
    let send_task = tokio::spawn(async move {
        while let Ok(data) = output_rx.recv().await {
            if sender.send(Message::Binary(data)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages using a channel to avoid Send issues
    let input_enabled = state.config.web.input_enabled;
    let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(32);

    // Task to process input
    let session_for_input = session.clone();
    let input_task = tokio::spawn(async move {
        while let Some(data) = input_rx.recv().await {
            let pty_guard = session_for_input.pty.read().await;
            if let Some(pty) = pty_guard.as_ref() {
                let _ = pty.write(&data).await;
            }
            drop(pty_guard);
        }
    });

    // Task to receive WebSocket messages
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if !input_enabled {
                continue;
            }

            match msg {
                Message::Text(text) => {
                    let _ = input_tx.send(text.into_bytes()).await;
                }
                Message::Binary(data) => {
                    let _ = input_tx.send(data).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
        _ = input_task => {},
    }
}
