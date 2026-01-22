//! Gemini log provider
//!
//! Observed storage layout:
//!   ~/.gemini/tmp/<project_hash>/chats/session-*.json
//!
//! Instead of computing project_hash (which is fragile due to path normalization
//! differences across platforms), we monitor the entire log directory and find
//! the most recently modified session file.

use super::{HistoryEntry, LockedSession, LogEntry, LogProvider, PathMapper};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

#[derive(Clone)]
struct LockedGeminiSession {
    path: PathBuf,
    baseline_timestamp: DateTime<Utc>,
}

pub struct GeminiLogProvider {
    log_path: PathBuf,
    current_offset: Arc<AtomicU64>,
    locked_session: Arc<Mutex<Option<LockedGeminiSession>>>,
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

        tracing::info!(
            "[GeminiLogProvider] Initialized with log_path={:?}",
            log_path
        );

        Self {
            log_path,
            current_offset: Arc::new(AtomicU64::new(0)),
            locked_session: Arc::new(Mutex::new(None)),
        }
    }

    fn default_timestamp() -> DateTime<Utc> {
        DateTime::<Utc>::from(SystemTime::UNIX_EPOCH)
    }

    fn default_log_path() -> PathBuf {
        if let Ok(root) = std::env::var("GEMINI_ROOT") {
            if !root.is_empty() {
                return PathMapper::normalize(&root);
            }
        }
        PathMapper::normalize("~/.gemini/tmp")
    }

    fn find_latest_chat_file(&self) -> Option<PathBuf> {
        tracing::debug!(
            "[GeminiLogProvider] Scanning for latest chat file in {:?}",
            self.log_path
        );

        if !self.log_path.exists() {
            tracing::warn!(
                "[GeminiLogProvider] Log path does not exist: {:?}",
                self.log_path
            );
            return None;
        }

        let result = Self::scan_latest_session(&self.log_path);
        if let Some(ref file) = result {
            tracing::debug!("[GeminiLogProvider] Found latest chat file: {:?}", file);
        } else {
            tracing::warn!("[GeminiLogProvider] No chat files found");
        }
        result
    }

    fn scan_latest_session(root: &PathBuf) -> Option<PathBuf> {
        let mut latest_file: Option<PathBuf> = None;
        let mut latest_time = std::time::SystemTime::UNIX_EPOCH;
        let mut project_count = 0u32;
        let mut total_files = 0u32;

        for entry in fs::read_dir(root).ok()?.filter_map(|e| e.ok()) {
            let hash_dir = entry.path();
            let chats_dir = hash_dir.join("chats");

            if !chats_dir.is_dir() {
                continue;
            }

            project_count += 1;

            let Ok(chat_entries) = fs::read_dir(&chats_dir) else {
                continue;
            };
            for chat_entry in chat_entries.filter_map(|e| e.ok()) {
                let path = chat_entry.path();
                let name = path.file_name().map(|n| n.to_string_lossy().to_string());

                if let Some(ref n) = name {
                    if !n.starts_with("session-") || !n.ends_with(".json") {
                        continue;
                    }
                }

                total_files += 1;

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

        tracing::debug!(
            "[GeminiLogProvider] Scanned {} project dirs, {} total files, latest: {:?}",
            project_count,
            total_files,
            latest_file
        );

        latest_file
    }

    fn scan_latest_session_file_in_chats_dir(chats_dir: &Path) -> Option<PathBuf> {
        let mut latest_file: Option<PathBuf> = None;
        let mut latest_time = std::time::SystemTime::UNIX_EPOCH;

        let entries = fs::read_dir(chats_dir).ok()?;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string());

            if let Some(ref n) = name {
                if !n.starts_with("session-") || !n.ends_with(".json") {
                    continue;
                }
            }

            if let Ok(metadata) = path.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified > latest_time {
                        latest_time = modified;
                        latest_file = Some(path);
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
                // Gemini uses "type" instead of "role"
                let role = msg.get("type")?.as_str()?.to_string();
                let content = msg.get("content")?.as_str()?.to_string();
                let timestamp = msg
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(Self::default_timestamp);
                Some((role, content, timestamp))
            })
            .collect()
    }

    fn is_assistant_role(role: &str) -> bool {
        role == "assistant" || role == "model" || role == "gemini"
    }

    fn read_file_to_string(path: &PathBuf) -> std::io::Result<String> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut content = String::new();
        reader.read_to_string(&mut content)?;
        Ok(content)
    }

    fn find_assistant_reply(
        &self,
        entries: &[(String, String, DateTime<Utc>)],
        since_offset: u64,
    ) -> Option<LogEntry> {
        // On 32-bit systems, u64 may overflow usize. Using MAX ensures .min() clamps
        // to array length, resulting in an empty slice (no matches) - correct behavior.
        let start = usize::try_from(since_offset)
            .unwrap_or(usize::MAX)
            .min(entries.len());

        // Use slice for O(1) offset instead of O(N) skip
        entries[start..]
            .iter()
            .enumerate()
            .rfind(|(_, (role, content, _))| {
                Self::is_assistant_role(role) && !content.trim().is_empty()
            })
            .map(|(local_idx, (_, content, timestamp))| LogEntry {
                content: content.clone(),
                offset: (start + local_idx) as u64 + 1,
                timestamp: *timestamp,
                inode: self.get_inode(),
            })
    }

    fn find_assistant_reply_by_timestamp(
        &self,
        entries: &[(String, String, DateTime<Utc>)],
        baseline_timestamp: DateTime<Utc>,
    ) -> Option<LogEntry> {
        entries
            .iter()
            .enumerate()
            .rfind(|(_, (role, content, timestamp))| {
                Self::is_assistant_role(role)
                    && *timestamp > baseline_timestamp
                    && !content.trim().is_empty()
            })
            .map(|(idx, (_, content, timestamp))| LogEntry {
                content: content.clone(),
                offset: idx as u64 + 1,
                timestamp: *timestamp,
                inode: self.get_inode(),
            })
    }

    fn scan_newer_session(
        &self,
        locked_file: &PathBuf,
        since_offset: u64,
        baseline_timestamp: DateTime<Utc>,
        default_timestamp: DateTime<Utc>,
    ) -> Option<(LogEntry, u64)> {
        let chats_dir = locked_file.parent()?;
        let latest_file = Self::scan_latest_session_file_in_chats_dir(chats_dir)?;

        if latest_file == *locked_file {
            return None;
        }

        tracing::info!(
            "[GeminiLogProvider] Found newer session file: {:?}, switching to it",
            latest_file
        );

        let content = match Self::read_file_to_string(&latest_file) {
            Ok(c) => c,
            Err(_) => {
                tracing::warn!(
                    "[GeminiLogProvider] Failed to read newer chat file: {:?}",
                    latest_file
                );
                return None;
            }
        };

        let entries = self.parse_chat_json(&content);
        let total_messages = entries.len() as u64;

        tracing::debug!(
            "[GeminiLogProvider] Parsed {} messages from newer file",
            total_messages
        );

        // First try offset-based search
        if let Some(entry) = self.find_assistant_reply(&entries, since_offset) {
            return Some((entry, total_messages));
        }

        // Fall back to timestamp-based search if baseline is valid
        if baseline_timestamp != default_timestamp {
            if let Some(entry) =
                self.find_assistant_reply_by_timestamp(&entries, baseline_timestamp)
            {
                return Some((entry, total_messages));
            }
        }

        None
    }
}

#[async_trait]
impl LogProvider for GeminiLogProvider {
    async fn get_latest_reply(&self, since_offset: u64) -> Option<LogEntry> {
        tracing::debug!(
            "[GeminiLogProvider] get_latest_reply called with since_offset={}",
            since_offset
        );

        let default_timestamp = Self::default_timestamp();

        // Use locked session file if available, otherwise find latest
        let (chat_file, baseline_timestamp, should_check_newer) = {
            let locked = self.locked_session.lock().await.clone();
            if let Some(ref locked) = locked {
                tracing::debug!(
                    "[GeminiLogProvider] Using locked session file: {:?}",
                    locked.path
                );
                (locked.path.clone(), locked.baseline_timestamp, true)
            } else {
                match self.find_latest_chat_file() {
                    Some(f) => {
                        tracing::debug!("[GeminiLogProvider] Found latest chat file: {:?}", f);
                        (f, default_timestamp, false)
                    }
                    None => {
                        tracing::debug!("[GeminiLogProvider] No chat file found");
                        return None;
                    }
                }
            }
        };

        // Try to find reply in current/locked session
        if let Ok(content) = Self::read_file_to_string(&chat_file) {
            let entries = self.parse_chat_json(&content);
            let total_messages = entries.len() as u64;

            tracing::debug!(
                "[GeminiLogProvider] Parsed {} messages from {:?}, since_offset={}",
                total_messages,
                chat_file,
                since_offset
            );

            if let Some(result) = self.find_assistant_reply(&entries, since_offset) {
                self.current_offset.store(total_messages, Ordering::SeqCst);
                tracing::info!(
                    "[GeminiLogProvider] Found assistant reply, new offset={}",
                    total_messages
                );
                return Some(result);
            }
        } else if !should_check_newer {
            tracing::warn!(
                "[GeminiLogProvider] Failed to read chat file {:?}, no fallback available",
                chat_file
            );
            return None;
        }

        // Fallback: check newer session file if we were using a locked session
        if should_check_newer {
            tracing::debug!(
                "[GeminiLogProvider] No reply in locked session, checking for newer files"
            );

            if let Some((result, total_messages)) = self.scan_newer_session(
                &chat_file,
                since_offset,
                baseline_timestamp,
                default_timestamp,
            ) {
                self.current_offset.store(total_messages, Ordering::SeqCst);
                tracing::info!(
                    "[GeminiLogProvider] Found assistant reply in newer file, offset={}",
                    total_messages
                );
                return Some(result);
            }
        }

        tracing::debug!("[GeminiLogProvider] No new assistant reply found");
        None
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
                // Normalize role: "gemini" and "model" -> "assistant"
                let normalized_role = if role == "model" || role == "gemini" {
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

    async fn lock_session(&self) -> Option<LockedSession> {
        let chat_file = self.find_latest_chat_file()?;
        let content = fs::read_to_string(&chat_file).ok()?;
        let entries = self.parse_chat_json(&content);
        let baseline_offset = entries.len() as u64;
        let default_timestamp = Self::default_timestamp();
        let baseline_timestamp = entries
            .iter()
            .rev()
            .map(|(_, _, ts)| *ts)
            .find(|ts| *ts != default_timestamp)
            .unwrap_or(default_timestamp);

        // Store locked session
        *self.locked_session.lock().await = Some(LockedGeminiSession {
            path: chat_file.clone(),
            baseline_timestamp,
        });

        tracing::info!(
            "[GeminiLogProvider] Session locked: {:?}, baseline_offset={}",
            chat_file,
            baseline_offset
        );

        Some(LockedSession {
            file_path: chat_file,
            baseline_offset,
        })
    }

    async fn unlock_session(&self) {
        let mut locked = self.locked_session.lock().await;
        if locked.is_some() {
            tracing::debug!("[GeminiLogProvider] Session unlocked");
        }
        *locked = None;
    }
}
