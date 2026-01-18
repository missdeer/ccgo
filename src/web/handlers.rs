//! HTTP handlers

use super::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub name: String,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub agents: Vec<AgentStatus>,
}

pub async fn api_get_status(
    State(state): State<AppState>,
) -> Result<Json<StatusResponse>, StatusCode> {
    let statuses = state.session_manager.get_all_status().await;

    let agents = statuses
        .into_iter()
        .map(|(name, s)| AgentStatus {
            name,
            state: s.to_string(),
        })
        .collect();

    Ok(Json(StatusResponse { agents }))
}

#[derive(Debug, Deserialize)]
pub struct StartAgentRequest {
    pub working_dir: Option<String>,
}

pub async fn api_start_agent(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = state
        .session_manager
        .get(&name)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    session
        .start(state.session_manager.pty_manager())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "status": "started",
        "agent": name
    })))
}

#[derive(Debug, Deserialize)]
pub struct StopAgentRequest {
    pub force: Option<bool>,
}

pub async fn api_stop_agent(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = state
        .session_manager
        .get(&name)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    let pty_manager = state.session_manager.pty_manager();
    session
        .stop(false, Some(pty_manager.as_ref()))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "status": "stopped",
        "agent": name
    })))
}

pub async fn api_interrupt_agent(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let session = state
        .session_manager
        .get(&name)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    session
        .interrupt()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({
        "status": "interrupted",
        "agent": name
    })))
}
