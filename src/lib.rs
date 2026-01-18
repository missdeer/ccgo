//! CCGO - ClaudeCode-Codex-Gemini-OpenCode MCP Server
//!
//! A multi-AI collaboration bridge based on MCP protocol.

pub mod agent;
pub mod config;
pub mod log_provider;
pub mod mcp;
pub mod pty;
pub mod session;
pub mod state;
pub mod web;

pub use config::Config;
pub use mcp::McpServer;
pub use session::SessionManager;
pub use web::WebServer;
