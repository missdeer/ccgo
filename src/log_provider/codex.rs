//! Codex log provider

use super::{HistoryEntry, LogEntry, LogProvider, PathMapper};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct CodexLogProvider {
    log_path: PathBuf,
    current_offset: Arc<AtomicU64>,
    #[allow(dead_code)]
    file_handle: Arc<Mutex<Option<File>>>,
}

impl CodexLogProvider {
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
            file_handle: Arc::new(Mutex::new(None)),
        }
    }

    fn default_log_path() -> PathBuf {
        PathMapper::normalize("~/.codex/sessions")
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
                    .map(|ext| ext == "jsonl")
                    .unwrap_or(false)
            })
            .collect();

        entries
            .into_iter()
            .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
            .map(|e| e.path())
    }

    fn parse_jsonl_entry(&self, line: &str) -> Option<(String, String, DateTime<Utc>)> {
        let json: serde_json::Value = serde_json::from_str(line).ok()?;

        let role = json.get("role")?.as_str()?.to_string();
        let content = json.get("content")?.as_str()?.to_string();
        let timestamp = json
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        Some((role, content, timestamp))
    }
}

#[async_trait]
impl LogProvider for CodexLogProvider {
    async fn get_latest_reply(&self, since_offset: u64) -> Option<LogEntry> {
        let session_file = self.find_latest_session_file()?;

        let file = File::open(&session_file).ok()?;
        let mut reader = BufReader::new(file);

        reader.seek(SeekFrom::Start(since_offset)).ok()?;

        let mut last_assistant_entry: Option<LogEntry> = None;
        let mut last_end_offset: u64 = since_offset;

        loop {
            let mut line = String::new();

            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    // Get position after reading line (end of line)
                    let end_pos = reader.stream_position().ok()?;

                    if let Some((role, content, timestamp)) = self.parse_jsonl_entry(&line) {
                        if role == "assistant" {
                            last_assistant_entry = Some(LogEntry {
                                content,
                                offset: end_pos, // Use end position to avoid re-reading
                                timestamp,
                                inode: self.get_inode(),
                            });
                            last_end_offset = end_pos;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        if last_assistant_entry.is_some() {
            self.current_offset.store(last_end_offset, Ordering::SeqCst);
        }

        last_assistant_entry
    }

    async fn get_history(&self, _session_id: Option<&str>, count: usize) -> Vec<HistoryEntry> {
        let Some(session_file) = self.find_latest_session_file() else {
            return Vec::new();
        };

        let Ok(content) = fs::read_to_string(&session_file) else {
            return Vec::new();
        };

        let mut entries: Vec<HistoryEntry> = content
            .lines()
            .filter_map(|line| {
                let (role, content, timestamp) = self.parse_jsonl_entry(line)?;
                Some(HistoryEntry {
                    role,
                    content,
                    timestamp,
                })
            })
            .collect();

        entries.reverse();
        entries.truncate(count);
        entries.reverse();

        entries
    }

    async fn get_current_offset(&self) -> u64 {
        if let Some(session_file) = self.find_latest_session_file() {
            if let Ok(metadata) = fs::metadata(&session_file) {
                return metadata.len();
            }
        }
        0
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
