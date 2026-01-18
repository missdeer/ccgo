//! Configuration module for ccgo

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub agents: HashMap<String, AgentConfig>,
    pub timeouts: TimeoutConfig,
    pub web: WebConfig,
}

impl Default for Config {
    fn default() -> Self {
        let mut agents = HashMap::new();

        agents.insert(
            "codex".to_string(),
            AgentConfig {
                command: "codex".to_string(),
                args: vec![],
                log_provider: "codex".to_string(),
            },
        );

        agents.insert(
            "gemini".to_string(),
            AgentConfig {
                command: "gemini".to_string(),
                args: vec![],
                log_provider: "gemini".to_string(),
            },
        );

        agents.insert(
            "opencode".to_string(),
            AgentConfig {
                command: "opencode".to_string(),
                args: vec![],
                log_provider: "opencode".to_string(),
            },
        );

        Self {
            server: ServerConfig::default(),
            agents,
            timeouts: TimeoutConfig::default(),
            web: WebConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub host: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 8765,
            host: "127.0.0.1".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub command: String,
    pub args: Vec<String>,
    pub log_provider: String,
}

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub default: u64,
    pub startup: u64,
    pub ready_check: u64,
    pub queue_wait: u64,
    pub max_stuck_duration: u64,
    pub max_start_retries: u32,
    pub start_retry_delay_ms: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            default: 600,
            startup: 30,
            ready_check: 30,
            queue_wait: 60,
            max_stuck_duration: 300,
            max_start_retries: 3,
            start_retry_delay_ms: 1000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebConfig {
    pub auth_token: Option<String>,
    pub input_enabled: bool,
    pub output_buffer_size: usize,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            auth_token: None,
            input_enabled: false,
            output_buffer_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl Config {
    pub fn get_agent(&self, name: &str) -> Option<&AgentConfig> {
        self.agents.get(name)
    }
}
