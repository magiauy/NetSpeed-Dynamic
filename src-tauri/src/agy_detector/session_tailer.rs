use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

use super::types::RawAgyRecord;

/// Events discovered from Antigravity transcript stream
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AgyStreamEvent {
    UserInput {
        step_index: Option<u64>,
        timestamp_ms: u64,
    },
    ThinkingStarted {
        step_index: Option<u64>,
        thinking_preview: Option<String>,
        timestamp_ms: u64,
    },
    ToolCallStarted {
        step_index: Option<u64>,
        tool_name: String,
        detail: String,
        timestamp_ms: u64,
    },
    WaitingApproval {
        step_index: Option<u64>,
        prompt: Option<String>,
        timestamp_ms: u64,
    },
    TurnCompleted {
        step_index: Option<u64>,
        timestamp_ms: u64,
    },
}

pub struct AgySessionTailer {
    active_conversation_id: Option<String>,
    current_transcript_path: Option<PathBuf>,
    file_offset: u64,
    is_initial_catchup: bool,
}

impl AgySessionTailer {
    pub fn new() -> Self {
        Self {
            active_conversation_id: None,
            current_transcript_path: None,
            file_offset: 0,
            is_initial_catchup: true,
        }
    }

    /// Poll for new presence / transcript file updates and emit stream events
    pub fn poll_events(&mut self, tx: &mpsc::UnboundedSender<AgyStreamEvent>) {
        let latest_conv = find_active_conversation_id();
        let target_path = latest_conv.as_ref().and_then(|id| get_transcript_path_for_conv(id));

        if self.active_conversation_id != latest_conv || self.current_transcript_path != target_path {
            self.active_conversation_id = latest_conv;
            self.current_transcript_path = target_path.clone();
            self.file_offset = 0;
            self.is_initial_catchup = true;
        }

        let transcript_file = match &self.current_transcript_path {
            Some(p) if p.exists() => p,
            _ => return,
        };

        let file = match File::open(transcript_file) {
            Ok(f) => f,
            Err(_) => return,
        };

        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(_) => return,
        };

        let file_len = metadata.len();

        // File truncated
        if file_len < self.file_offset {
            self.file_offset = 0;
        }

        // On startup or session switch, ignore the existing transcript history.
        // Replaying a stale Thinking/Executing entry (for example after quota exhaustion)
        // incorrectly marks the Dynamic Island as active until the inactivity timeout fires.
        if self.is_initial_catchup {
            self.is_initial_catchup = false;
            self.file_offset = file_len;
            return;
        }

        if file_len == self.file_offset {
            return;
        }

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(self.file_offset)).is_err() {
            return;
        }

        let mut line_buf = String::new();
        let now_unix = current_unix_ms();

        while let Ok(bytes_read) = reader.read_line(&mut line_buf) {
            if bytes_read == 0 {
                break;
            }
            self.file_offset += bytes_read as u64;

            let trimmed = line_buf.trim();
            if !trimmed.is_empty() {
                if let Ok(record) = serde_json::from_str::<RawAgyRecord>(trimmed) {
                    process_record(&record, now_unix, tx);
                }
            }
            line_buf.clear();
        }
    }
}

fn process_record(
    record: &RawAgyRecord,
    now_unix: u64,
    tx: &mpsc::UnboundedSender<AgyStreamEvent>,
) {
    let step_idx = record.step_index;
    let rec_type = record.record_type.as_deref().unwrap_or_default();
    let source = record.source.as_deref().unwrap_or_default();
    let status = record.status.as_deref().unwrap_or_default();

    if rec_type == "USER_INPUT" || source == "USER_EXPLICIT" {
        let _ = tx.send(AgyStreamEvent::UserInput {
            step_index: step_idx,
            timestamp_ms: now_unix,
        });
        return;
    }

    if let Some(tool_calls) = &record.tool_calls {
        if !tool_calls.is_empty() {
            for tc in tool_calls {
                let name = tc.name.clone().unwrap_or_else(|| "tool".to_string());
                let detail = format_tool_detail(&name, tc.args.as_ref());

                if name == "ask_question" {
                    let _ = tx.send(AgyStreamEvent::WaitingApproval {
                        step_index: step_idx,
                        prompt: Some(detail.clone()),
                        timestamp_ms: now_unix,
                    });
                } else {
                    let _ = tx.send(AgyStreamEvent::ToolCallStarted {
                        step_index: step_idx,
                        tool_name: name,
                        detail,
                        timestamp_ms: now_unix,
                    });
                }
            }
            return;
        }
    }

    if rec_type == "PLANNER_RESPONSE" && status == "DONE" {
        let _ = tx.send(AgyStreamEvent::TurnCompleted {
            step_index: step_idx,
            timestamp_ms: now_unix,
        });
        return;
    }

    if let Some(thinking) = &record.thinking {
        if !thinking.trim().is_empty() {
            let preview = if thinking.len() > 60 {
                format!("{}...", &thinking[..60])
            } else {
                thinking.clone()
            };
            let _ = tx.send(AgyStreamEvent::ThinkingStarted {
                step_index: step_idx,
                thinking_preview: Some(preview),
                timestamp_ms: now_unix,
            });
            return;
        }
    }
}

fn format_tool_detail(name: &str, args: Option<&serde_json::Value>) -> String {
    match name {
        "run_command" => {
            if let Some(cmd) = args.and_then(|a| a.get("CommandLine")).and_then(|v| v.as_str()) {
                let short = cmd.trim();
                let short = if short.len() > 36 {
                    format!("{}...", &short[..36])
                } else {
                    short.to_string()
                };
                format!("Lệnh: {}", short)
            } else {
                "Chạy lệnh terminal".to_string()
            }
        }
        "replace_file_content" | "write_to_file" => {
            if let Some(file) = args.and_then(|a| a.get("TargetFile")).and_then(|v| v.as_str()) {
                let p = std::path::Path::new(file);
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or(file);
                format!("Sửa file: {}", name)
            } else {
                "Chỉnh sửa mã nguồn".to_string()
            }
        }
        "view_file" => {
            if let Some(file) = args.and_then(|a| a.get("AbsolutePath")).and_then(|v| v.as_str()) {
                let p = std::path::Path::new(file);
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or(file);
                format!("Đọc file: {}", name)
            } else {
                "Đọc mã nguồn".to_string()
            }
        }
        "grep_search" | "find_by_name" => "Tìm kiếm trong dự án".to_string(),
        "search_web" | "read_url_content" => "Tìm kiếm web / tài liệu".to_string(),
        "ask_question" => "Chờ bạn phản hồi / phê duyệt".to_string(),
        other => format!("Thực thi {}", other),
    }
}

fn get_gemini_cli_dir() -> Option<PathBuf> {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(profile).join(".gemini").join("antigravity-cli");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".gemini").join("antigravity-cli");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Find active conversation ID from presence locks or recent brain folders
fn find_active_conversation_id() -> Option<String> {
    let cli_dir = get_gemini_cli_dir()?;

    // 1. Check presence lock files first (100% accurate for active sessions)
    let presence_dir = cli_dir.join("presence");
    if presence_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&presence_dir) {
            let mut newest_lock: Option<(String, SystemTime)> = None;
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    if ext == "lock" {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            if let Ok(meta) = entry.metadata() {
                                if let Ok(modified) = meta.modified() {
                                    if newest_lock.as_ref().map_or(true, |(_, t)| modified > *t) {
                                        newest_lock = Some((stem.to_string(), modified));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some((id, _)) = newest_lock {
                return Some(id);
            }
        }
    }

    // 2. Fallback: Check brain subdirectories sorted by last write time
    let brain_dir = cli_dir.join("brain");
    if brain_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&brain_dir) {
            let mut newest_folder: Option<(String, SystemTime)> = None;
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        if let Some(name) = entry.file_name().to_str() {
                            if let Ok(modified) = meta.modified() {
                                if newest_folder.as_ref().map_or(true, |(_, t)| modified > *t) {
                                    newest_folder = Some((name.to_string(), modified));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((id, _)) = newest_folder {
                return Some(id);
            }
        }
    }

    None
}

fn get_transcript_path_for_conv(conv_id: &str) -> Option<PathBuf> {
    let cli_dir = get_gemini_cli_dir()?;
    let path = cli_dir
        .join("brain")
        .join(conv_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");

    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
