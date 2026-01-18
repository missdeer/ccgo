//! OpenCode log provider

use super::{HistoryEntry, LogEntry, LogProvider, PathMapper};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct OpenCodeLogProvider {
    log_path: PathBuf,
    current_offset: Arc<AtomicU64>,
}

impl OpenCodeLogProvider {
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
        PathMapper::normalize("~/.local/share/opencode/storage")
    }

    fn find_latest_session_file(&self) -> Option<PathBuf> {
        if !self.log_path.exists() {
            return None;
        }

        let entries: Vec<_> = fs::read_dir(&self.log_path)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .collect();

        entries
            .into_iter()
            .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
            .map(|e| e.path())
    }

    fn parse_session_json(&self, content: &str) -> Vec<(String, String, DateTime<Utc>)> {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(content) else {
            return Vec::new();
        };

        let Some(messages) = json.get("messages").and_then(|m| m.as_array()) else {
            // Try alternate structure
            if let Some(turns) = json.get("turns").and_then(|t| t.as_array()) {
                return turns
                    .iter()
                    .filter_map(|turn| {
                        let role = turn.get("role")?.as_str()?.to_string();
                        let content = turn.get("content")?.as_str()?.to_string();
                        let timestamp = turn
                            .get("created_at")
                            .and_then(|t| t.as_str())
                            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                            .map(|t| t.with_timezone(&Utc))
                            .unwrap_or_else(Utc::now);
                        Some((role, content, timestamp))
                    })
                    .collect();
            }
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
impl LogProvider for OpenCodeLogProvider {
    async fn get_latest_reply(&self, since_offset: u64) -> Option<LogEntry> {
        let session_file = self.find_latest_session_file()?;

        let file = File::open(&session_file).ok()?;
        let mut reader = BufReader::new(file);
        let mut content = String::new();
        reader.read_to_string(&mut content).ok()?;

        let entries = self.parse_session_json(&content);
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
            .rfind(|(_, (role, _, _))| role == "assistant")
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
        let Some(session_file) = self.find_latest_session_file() else {
            return Vec::new();
        };

        let Ok(content) = fs::read_to_string(&session_file) else {
            return Vec::new();
        };

        let mut entries: Vec<HistoryEntry> = self
            .parse_session_json(&content)
            .into_iter()
            .map(|(role, content, timestamp)| HistoryEntry {
                role,
                content,
                timestamp,
            })
            .collect();

        entries.reverse();
        entries.truncate(count);
        entries.reverse();

        entries
    }

    async fn get_current_offset(&self) -> u64 {
        // Return current message count as offset
        if let Some(session_file) = self.find_latest_session_file() {
            if let Ok(content) = fs::read_to_string(&session_file) {
                return self.parse_session_json(&content).len() as u64;
            }
        }
        self.current_offset.load(Ordering::SeqCst)
    }

    fn get_inode(&self) -> Option<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.find_latest_session_file()
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
