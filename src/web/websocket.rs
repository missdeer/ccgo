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
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tracing::warn;

/// Minimum terminal size (columns or rows)
const MIN_TERMINAL_SIZE: u16 = 1;
/// Maximum terminal size (columns or rows)
const MAX_TERMINAL_SIZE: u16 = 500;
/// Interval to check for PTY availability when agent is not running
const PTY_POLL_INTERVAL_MS: u64 = 1000;

/// Control command from frontend (sent as JSON with \x00 prefix)
/// Note: Control commands (like resize) are intentionally allowed even when
/// input_enabled is false. This is because:
/// 1. Resize is not user input - it's terminal synchronization
/// 2. Correct terminal size is needed for proper output rendering
/// 3. Read-only viewers still need accurate terminal dimensions
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ControlCommand {
    Resize { cols: u16, rows: u16 },
}

/// Message types to send to PTY
enum PtyMessage {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(agent_name): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, agent_name))
}

async fn handle_socket(socket: WebSocket, state: AppState, agent_name: String) {
    let (mut sender, receiver) = socket.split();

    // Get the session for this agent
    let Some(session) = state.session_manager.get(&agent_name).await else {
        let _ = sender
            .send(Message::Text(format!(
                "Error: Agent {} not found",
                agent_name
            )))
            .await;
        return;
    };

    // Wait for PTY to become available (agent may not be started yet)
    let pty = loop {
        let pty_guard = session.pty.read().await;
        if let Some(pty) = pty_guard.as_ref() {
            break Arc::clone(pty);
        }
        drop(pty_guard);

        // Send waiting message to client
        let _ = sender
            .send(Message::Text(format!(
                "\x1b[33mWaiting for agent {} to start...\x1b[0m\r\n",
                agent_name
            )))
            .await;

        // Wait before checking again
        tokio::time::sleep(Duration::from_millis(PTY_POLL_INTERVAL_MS)).await;
    };

    // Send buffered output first
    let buffer = pty.get_buffer().await;
    if !buffer.is_empty() {
        let _ = sender.send(Message::Binary(buffer)).await;
    }

    // Subscribe to new output
    let output_rx = pty.subscribe_output();

    // Spawn task to forward PTY output to WebSocket
    let send_task = spawn_send_task(sender, output_rx);

    // Handle incoming messages using a channel to avoid Send issues
    let input_enabled = state.config.web.input_enabled;
    let (input_tx, input_rx) = mpsc::channel::<PtyMessage>(32);

    // Task to process input and control commands
    let input_task = spawn_input_task(session.clone(), input_rx);

    // Task to receive WebSocket messages
    let recv_task = spawn_recv_task(receiver, input_tx, input_enabled);

    // Wait for either task to complete
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
        _ = input_task => {},
    }
}

fn spawn_send_task(
    mut sender: futures_util::stream::SplitSink<WebSocket, Message>,
    mut output_rx: broadcast::Receiver<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(data) = output_rx.recv().await {
            if sender.send(Message::Binary(data)).await.is_err() {
                break;
            }
        }
    })
}

fn spawn_input_task(
    session: Arc<crate::session::AgentSession>,
    mut input_rx: mpsc::Receiver<PtyMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = input_rx.recv().await {
            let pty_guard = session.pty.read().await;
            if let Some(pty) = pty_guard.as_ref() {
                match msg {
                    PtyMessage::Input(data) => {
                        let _ = pty.write(&data).await;
                    }
                    PtyMessage::Resize { cols, rows } => {
                        let _ = pty.resize(cols, rows).await;
                    }
                }
            }
            drop(pty_guard);
        }
    })
}

fn spawn_recv_task(
    mut receiver: futures_util::stream::SplitStream<WebSocket>,
    input_tx: mpsc::Sender<PtyMessage>,
    input_enabled: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // Check for control command (starts with \x00)
                    if let Some(json_str) = text.strip_prefix('\x00') {
                        match serde_json::from_str::<ControlCommand>(json_str) {
                            Ok(cmd) => match cmd {
                                ControlCommand::Resize { cols, rows } => {
                                    // Clamp values to reasonable bounds
                                    let cols = cols.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
                                    let rows = rows.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
                                    let _ = input_tx.send(PtyMessage::Resize { cols, rows }).await;
                                }
                            },
                            Err(e) => {
                                warn!("Invalid control command: {}", e);
                            }
                        }
                    } else if input_enabled {
                        let _ = input_tx.send(PtyMessage::Input(text.into_bytes())).await;
                    }
                }
                Message::Binary(data) => {
                    if input_enabled {
                        let _ = input_tx.send(PtyMessage::Input(data)).await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_command_resize_parsing() {
        let json = r#"{"type":"resize","cols":80,"rows":24}"#;
        let cmd: ControlCommand = serde_json::from_str(json).unwrap();
        match cmd {
            ControlCommand::Resize { cols, rows } => {
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
        }
    }

    #[test]
    fn test_control_command_invalid_type() {
        let json = r#"{"type":"unknown","cols":80,"rows":24}"#;
        let result = serde_json::from_str::<ControlCommand>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_control_command_missing_fields() {
        let json = r#"{"type":"resize","cols":80}"#;
        let result = serde_json::from_str::<ControlCommand>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_resize_value_clamping() {
        // Test minimum clamping
        let cols: u16 = 0;
        let rows: u16 = 0;
        let clamped_cols = cols.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
        let clamped_rows = rows.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
        assert_eq!(clamped_cols, MIN_TERMINAL_SIZE);
        assert_eq!(clamped_rows, MIN_TERMINAL_SIZE);

        // Test maximum clamping
        let cols: u16 = 1000;
        let rows: u16 = 1000;
        let clamped_cols = cols.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
        let clamped_rows = rows.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
        assert_eq!(clamped_cols, MAX_TERMINAL_SIZE);
        assert_eq!(clamped_rows, MAX_TERMINAL_SIZE);

        // Test normal values
        let cols: u16 = 120;
        let rows: u16 = 40;
        let clamped_cols = cols.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
        let clamped_rows = rows.clamp(MIN_TERMINAL_SIZE, MAX_TERMINAL_SIZE);
        assert_eq!(clamped_cols, 120);
        assert_eq!(clamped_rows, 40);
    }
}
