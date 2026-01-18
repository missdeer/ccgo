//! Gemini log provider

use super::{HistoryEntry, LogEntry, LogProvider, PathMapper};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct GeminiLogProvider {
    log_path: PathBuf,
    current_offset: Arc<AtomicU64>,
}

impl GeminiLogProvider {
    pub fn new(config: Option<&HashMap<String, String>>) -> Self {
        let log_path = if let Some(cfg) = config {
            if let Some(path) = cfg.get("path_pattern") {
                PathMapper::normalize(path)
            } else {
                Self::default_log_path()
            }
        } else {
            Self::default_log_path()
        };

        Self {
            log_path,
            current_offset: Arc::new(AtomicU64::new(0)),
        }
    }

    fn default_log_path() -> PathBuf {
        PathMapper::normalize("~/.gemini/tmp")
    }

    fn find_latest_chat_file(&self) -> Option<PathBuf> {
        if !self.log_path.exists() {
            return None;
        }

        // Gemini stores chats in ~/.gemini/tmp/<hash>/chats/
        let mut latest_file: Option<PathBuf> = None;
        let mut latest_time = std::time::SystemTime::UNIX_EPOCH;

        for entry in fs::read_dir(&self.log_path).ok()?.filter_map(|e| e.ok()) {
            let hash_dir = entry.path();
            let chats_dir = hash_dir.join("chats");

            if chats_dir.is_dir() {
                for chat_entry in fs::read_dir(&chats_dir).ok()?.filter_map(|e| e.ok()) {
                    let path = chat_entry.path();
                    if path.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Ok(metadata) = path.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                if modified > latest_time {
                                    latest_time = modified;
                                    latest_file = Some(path);
                                }
                            }
                        }
                    }
                }
            }
        }

        latest_file
    }

    fn parse_chat_json(&self, content: &str) -> Vec<(String, String, DateTime<Utc>)> {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(content) else {
            return Vec::new();
        };

        let Some(messages) = json.get("messages").and_then(|m| m.as_array()) else {
            return Vec::new();
        };

        messages
            .iter()
            .filter_map(|msg| {
                let role = msg.get("role")?.as_str()?.to_string();
                let content = msg.get("content")?.as_str()?.to_string();
                let timestamp = msg
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);
                Some((role, content, timestamp))
            })
            .collect()
    }
}

#[async_trait]
impl LogProvider for GeminiLogProvider {
    async fn get_latest_reply(&self, since_offset: u64) -> Option<LogEntry> {
        let chat_file = self.find_latest_chat_file()?;

        let file = File::open(&chat_file).ok()?;
        let mut reader = BufReader::new(file);
        let mut content = String::new();
        reader.read_to_string(&mut content).ok()?;

        let entries = self.parse_chat_json(&content);
        let total_messages = entries.len() as u64;

        // Only consider messages after since_offset (message index)
        let new_entries: Vec<_> = entries
            .into_iter()
            .enumerate()
            .filter(|(idx, _)| *idx as u64 >= since_offset)
            .collect();

        // Find last assistant message in new entries
        let result = new_entries
            .into_iter()
            .rfind(|(_, (role, _, _))| role == "assistant" || role == "model")
            .map(|(idx, (_, content, timestamp))| LogEntry {
                content,
                offset: idx as u64 + 1, // Next message index
                timestamp,
                inode: self.get_inode(),
            });

        // Update current offset to total message count
        if result.is_some() {
            self.current_offset.store(total_messages, Ordering::SeqCst);
        }

        result
    }

    async fn get_history(&self, _session_id: Option<&str>, count: usize) -> Vec<HistoryEntry> {
        let Some(chat_file) = self.find_latest_chat_file() else {
            return Vec::new();
        };

        let Ok(content) = fs::read_to_string(&chat_file) else {
            return Vec::new();
        };

        let mut entries: Vec<HistoryEntry> = self
            .parse_chat_json(&content)
            .into_iter()
            .map(|(role, content, timestamp)| {
                let normalized_role = if role == "model" {
                    "assistant".to_string()
                } else {
                    role
                };
                HistoryEntry {
                    role: normalized_role,
                    content,
                    timestamp,
                }
            })
            .collect();

        entries.reverse();
        entries.truncate(count);
        entries.reverse();

        entries
    }

    async fn get_current_offset(&self) -> u64 {
        // Return current message count as offset
        if let Some(chat_file) = self.find_latest_chat_file() {
            if let Ok(content) = fs::read_to_string(&chat_file) {
                return self.parse_chat_json(&content).len() as u64;
            }
        }
        self.current_offset.load(Ordering::SeqCst)
    }

    fn get_inode(&self) -> Option<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.find_latest_chat_file()
                .and_then(|p| fs::metadata(&p).ok())
                .map(|m| m.ino())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    fn get_watch_path(&self) -> Option<PathBuf> {
        Some(self.log_path.clone())
    }
}
