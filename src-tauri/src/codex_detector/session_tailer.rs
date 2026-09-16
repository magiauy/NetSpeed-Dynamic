use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

use super::types::{RawPayload, RawSessionRecord};

/// Discovered raw event from session jsonl stream
#[derive(Debug, Clone)]
pub enum SessionStreamEvent {
    TaskStarted {
        turn_id: Option<String>,
        session_id: Option<String>,
        timestamp_ms: u64,
    },
    ReasoningStarted {
        turn_id: Option<String>,
        timestamp_ms: u64,
    },
    ToolCallStarted {
        turn_id: Option<String>,
        name: String,
        input_preview: Option<String>,
        timestamp_ms: u64,
    },
    ItemCompleted {
        turn_id: Option<String>,
        timestamp_ms: u64,
    },
    TaskCompleted {
        turn_id: Option<String>,
        duration_ms: Option<u64>,
        timestamp_ms: u64,
    },
    TurnAborted {
        turn_id: Option<String>,
        error: Option<String>,
        timestamp_ms: u64,
    },
}

pub struct SessionTailer {
    sessions_root: Option<PathBuf>,
    current_file: Option<PathBuf>,
    last_offset: u64,
    last_file_mtime: Option<SystemTime>,
}

impl SessionTailer {
    pub fn new() -> Self {
        let root = get_codex_sessions_root();
        Self {
            sessions_root: root,
            current_file: None,
            last_offset: 0,
            last_file_mtime: None,
        }
    }

    /// Process incremental lines and send parsed events to the channel
    pub fn poll_events(&mut self, tx: &mpsc::UnboundedSender<SessionStreamEvent>) {
        if self.sessions_root.is_none() {
            self.sessions_root = get_codex_sessions_root();
            if self.sessions_root.is_none() {
                return;
            }
        }

        let root = self.sessions_root.as_ref().unwrap();
        let newest_file = find_latest_session_file(root);

        if let Some((latest_path, mtime)) = newest_file {
            let is_new_file = match &self.current_file {
                Some(cur) => cur != &latest_path,
                None => true,
            };

            if is_new_file {
                self.current_file = Some(latest_path.clone());
                self.last_file_mtime = Some(mtime);

                // For a newly opened file, if it's already large (e.g. older session),
                // seek close to the end or inspect recent lines to hydrate state
                let file_len = fs::metadata(&latest_path).map(|m| m.len()).unwrap_or(0);
                let seek_back = 32 * 1024u64; // Read last 32KB to catch active turn context
                self.last_offset = file_len.saturating_sub(seek_back);
            }

            self.read_incremental_lines(&latest_path, tx);
        }
    }

    fn read_incremental_lines(
        &mut self,
        path: &Path,
        tx: &mpsc::UnboundedSender<SessionStreamEvent>,
    ) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return,
        };

        let current_len = metadata.len();
        if current_len < self.last_offset {
            // File truncated or rewritten
            self.last_offset = 0;
        } else if current_len == self.last_offset {
            // No new data
            return;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.last_offset)).is_err() {
            return;
        }

        let session_id = extract_session_id_from_path(path);
        let mut line = String::new();

        while let Ok(bytes_read) = reader.read_line(&mut line) {
            if bytes_read == 0 {
                break;
            }

            // Only process complete lines ending with newline
            if line.ends_with('\n') {
                self.last_offset += bytes_read as u64;
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Some(event) = parse_jsonl_line(trimmed, session_id.as_deref()) {
                        let _ = tx.send(event);
                    }
                }
                line.clear();
            } else {
                // Incomplete line in buffer; keep offset for next poll
                break;
            }
        }
    }
}

/// Find the sessions directory under `.codex/sessions`
fn get_codex_sessions_root() -> Option<PathBuf> {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let path = PathBuf::from(profile).join(".codex").join("sessions");
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(".codex").join("sessions");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Extract session UUID from rollout filename
fn extract_session_id_from_path(path: &Path) -> Option<String> {
    let file_stem = path.file_stem()?.to_string_lossy();
    let parts: Vec<&str> = file_stem.split('-').collect();
    if parts.len() >= 5 {
        // e.g. rollout-2026-09-16T20-12-27-<uuid>
        Some(parts[parts.len() - 1].to_string())
    } else {
        Some(file_stem.to_string())
    }
}

/// Recursively find newest `.jsonl` file in `.codex/sessions`
fn find_latest_session_file(root: &Path) -> Option<(PathBuf, SystemTime)> {
    let mut newest_file: Option<(PathBuf, SystemTime)> = None;

    fn walk_dir(dir: &Path, newest: &mut Option<(PathBuf, SystemTime)>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, newest);
            } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        match newest {
                            Some((_, cur_time)) if mtime > *cur_time => {
                                *newest = Some((path, mtime));
                            }
                            None => {
                                *newest = Some((path, mtime));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    walk_dir(root, &mut newest_file);
    newest_file
}

/// Parse one JSONL record into semantic event
fn parse_jsonl_line(line: &str, session_id: Option<&str>) -> Option<SessionStreamEvent> {
    let record: RawSessionRecord = serde_json::from_str(line).ok()?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let payload = record.payload.unwrap_or(RawPayload {
        payload_type: None,
        turn_id: None,
        thread_id: None,
        name: None,
        input: None,
        status: None,
        duration_ms: None,
        error: None,
    });

    let payload_type = payload.payload_type.as_deref().unwrap_or("");

    match record.record_type.as_str() {
        "event_msg" => match payload_type {
            "task_started" => Some(SessionStreamEvent::TaskStarted {
                turn_id: payload.turn_id,
                session_id: session_id.map(|s| s.to_string()),
                timestamp_ms: now_ms,
            }),
            "task_complete" => Some(SessionStreamEvent::TaskCompleted {
                turn_id: payload.turn_id,
                duration_ms: payload.duration_ms,
                timestamp_ms: now_ms,
            }),
            "item_completed" => Some(SessionStreamEvent::ItemCompleted {
                turn_id: payload.turn_id,
                timestamp_ms: now_ms,
            }),
            "turn_aborted" => Some(SessionStreamEvent::TurnAborted {
                turn_id: payload.turn_id,
                error: payload.error,
                timestamp_ms: now_ms,
            }),
            _ => None,
        },
        "response_item" => match payload_type {
            "custom_tool_call" | "function_call" => {
                let name = payload.name.unwrap_or_else(|| "tool_call".to_string());
                Some(SessionStreamEvent::ToolCallStarted {
                    turn_id: payload.turn_id,
                    name,
                    input_preview: payload.input,
                    timestamp_ms: now_ms,
                })
            }
            "reasoning" => Some(SessionStreamEvent::ReasoningStarted {
                turn_id: payload.turn_id,
                timestamp_ms: now_ms,
            }),
            _ => None,
        },
        _ => None,
    }
}
