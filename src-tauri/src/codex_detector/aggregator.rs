use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::session_tailer::SessionStreamEvent;
use super::types::{CodexActivityPayload, CodexState, ToolKind};

const REVIEW_HOLD_DURATION: Duration = Duration::from_millis(3000);
const FAILED_HOLD_DURATION: Duration = Duration::from_millis(4000);
const TOOL_GAP_SMOOTH_DURATION: Duration = Duration::from_millis(400);
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(120);

pub struct StateAggregator {
    current_payload: CodexActivityPayload,
    state_since_instant: Instant,
    last_event_instant: Instant,
    active_turn_id: Option<String>,
    pending_tool_gap: bool,
    gap_start_instant: Option<Instant>,
}

impl StateAggregator {
    pub fn new() -> Self {
        let now_inst = Instant::now();
        Self {
            current_payload: CodexActivityPayload::default(),
            state_since_instant: now_inst,
            last_event_instant: now_inst,
            active_turn_id: None,
            pending_tool_gap: false,
            gap_start_instant: None,
        }
    }

    pub fn current_payload(&self) -> &CodexActivityPayload {
        &self.current_payload
    }

    pub fn handle_event(&mut self, event: SessionStreamEvent) -> bool {
        let now_inst = Instant::now();
        let now_unix = current_unix_ms();
        self.last_event_instant = now_inst;

        let prev_state = self.current_payload.state;
        let prev_tool = self.current_payload.active_tool.clone();

        match event {
            SessionStreamEvent::TaskStarted {
                turn_id,
                session_id,
                timestamp_ms,
            } => {
                self.active_turn_id = turn_id.clone();
                self.pending_tool_gap = false;
                self.gap_start_instant = None;

                self.current_payload.state = CodexState::Thinking;
                self.current_payload.session_id = session_id;
                self.current_payload.turn_id = turn_id;
                self.current_payload.active_tool = Some(ToolKind::Reasoning);
                self.current_payload.detail_message = Some("Đang suy nghĩ...".to_string());
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.current_payload.confidence = 1.0;
                self.state_since_instant = now_inst;
            }

            SessionStreamEvent::ReasoningStarted {
                turn_id,
                timestamp_ms,
            } => {
                self.active_turn_id = turn_id.clone();
                self.pending_tool_gap = false;
                self.gap_start_instant = None;

                if self.current_payload.state != CodexState::Executing {
                    self.current_payload.state = CodexState::Thinking;
                    self.current_payload.turn_id = turn_id;
                    self.current_payload.active_tool = Some(ToolKind::Reasoning);
                    self.current_payload.detail_message = Some("Đang suy nghĩ...".to_string());
                    self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                }
            }

            SessionStreamEvent::ToolCallStarted {
                turn_id,
                name,
                input_preview,
                timestamp_ms,
            } => {
                self.active_turn_id = turn_id.clone();
                self.pending_tool_gap = false;
                self.gap_start_instant = None;

                let tool_kind = classify_tool(&name, input_preview.as_deref());
                let detail = format_tool_detail(&tool_kind, &name, input_preview.as_deref());

                self.current_payload.state = CodexState::Executing;
                self.current_payload.turn_id = turn_id;
                self.current_payload.active_tool = Some(tool_kind);
                self.current_payload.detail_message = Some(detail);
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }

            SessionStreamEvent::ItemCompleted { .. } => {
                if self.current_payload.state == CodexState::Executing {
                    self.pending_tool_gap = true;
                    self.gap_start_instant = Some(now_inst);
                }
            }

            SessionStreamEvent::TaskCompleted {
                turn_id,
                duration_ms,
                timestamp_ms,
            } => {
                self.active_turn_id = None;
                self.pending_tool_gap = false;
                self.gap_start_instant = None;

                let dur_str = duration_ms
                    .map(|d| format!(" ({:.1}s)", d as f64 / 1000.0))
                    .unwrap_or_default();

                self.current_payload.state = CodexState::Review;
                self.current_payload.turn_id = turn_id;
                self.current_payload.active_tool = None;
                self.current_payload.detail_message =
                    Some(format!("Tác vụ hoàn thành{}", dur_str));
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }

            SessionStreamEvent::TurnAborted {
                turn_id,
                error,
                timestamp_ms,
            } => {
                self.active_turn_id = None;
                self.pending_tool_gap = false;
                self.gap_start_instant = None;

                self.current_payload.state = CodexState::Failed;
                self.current_payload.turn_id = turn_id;
                self.current_payload.active_tool = None;
                self.current_payload.detail_message =
                    error.or_else(|| Some("Tác vụ bị hủy hoặc gặp lỗi".to_string()));
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }
        }

        self.current_payload.state != prev_state
            || self.current_payload.active_tool != prev_tool
    }

    /// Check timers (holds, gaps, timeouts) and return true if state changed
    pub fn tick(&mut self) -> bool {
        let now_inst = Instant::now();
        let prev_state = self.current_payload.state;

        // 1. Tool gap smoothing: Executing -> Thinking after small gap if turn active
        if self.pending_tool_gap {
            if let Some(gap_start) = self.gap_start_instant {
                if now_inst.duration_since(gap_start) >= TOOL_GAP_SMOOTH_DURATION {
                    self.pending_tool_gap = false;
                    self.gap_start_instant = None;
                    if self.active_turn_id.is_some() {
                        self.current_payload.state = CodexState::Thinking;
                        self.current_payload.active_tool = Some(ToolKind::Reasoning);
                        self.current_payload.detail_message =
                            Some("Đang xử lý tiếp...".to_string());
                    }
                }
            }
        }

        // 2. Review hold expiration
        if self.current_payload.state == CodexState::Review {
            if now_inst.duration_since(self.state_since_instant) >= REVIEW_HOLD_DURATION {
                self.reset_to_idle();
            }
        }

        // 3. Failed hold expiration
        if self.current_payload.state == CodexState::Failed {
            if now_inst.duration_since(self.state_since_instant) >= FAILED_HOLD_DURATION {
                self.reset_to_idle();
            }
        }

        // 4. Inactivity timeout (e.g. process killed or uncompleted turn)
        if (self.current_payload.state == CodexState::Thinking
            || self.current_payload.state == CodexState::Executing)
            && now_inst.duration_since(self.last_event_instant) >= INACTIVITY_TIMEOUT
        {
            self.reset_to_idle();
        }

        self.current_payload.state != prev_state
    }

    fn reset_to_idle(&mut self) {
        self.current_payload.state = CodexState::Idle;
        self.current_payload.active_tool = None;
        self.current_payload.detail_message = None;
        self.current_payload.turn_id = None;
        self.active_turn_id = None;
        self.pending_tool_gap = false;
        self.gap_start_instant = None;
        self.state_since_instant = Instant::now();
    }
}

fn classify_tool(name: &str, input: Option<&str>) -> ToolKind {
    let lower_name = name.to_lowercase();
    let lower_input = input.unwrap_or("").to_lowercase();

    if lower_name.contains("exec")
        || lower_name.contains("bash")
        || lower_name.contains("terminal")
        || lower_input.contains("exec_command")
    {
        ToolKind::CommandExecution
    } else if lower_name.contains("edit")
        || lower_name.contains("patch")
        || lower_name.contains("write")
        || lower_input.contains("replace_file")
    {
        ToolKind::FileEdit
    } else if lower_name.contains("mcp") || lower_name.starts_with("call_mcp") {
        ToolKind::McpCall
    } else if lower_name.contains("search") || lower_name.contains("browser") {
        ToolKind::WebSearch
    } else {
        ToolKind::Other(name.to_string())
    }
}

fn format_tool_detail(tool_kind: &ToolKind, name: &str, input: Option<&str>) -> String {
    match tool_kind {
        ToolKind::CommandExecution => {
            if let Some(inp) = input {
                if let Some(cmd) = extract_command_preview(inp) {
                    return format!("Chạy lệnh: {}", truncate_str(&cmd, 35));
                }
            }
            "Đang thực thi lệnh...".to_string()
        }
        ToolKind::FileEdit => "Đang chỉnh sửa tệp...".to_string(),
        ToolKind::McpCall => format!("MCP: {}", name),
        ToolKind::WebSearch => "Đang tìm kiếm...".to_string(),
        ToolKind::Reasoning => "Đang suy nghĩ...".to_string(),
        ToolKind::Other(s) => format!("Công cụ: {}", s),
    }
}

fn extract_command_preview(input: &str) -> Option<String> {
    if let Some(start) = input.find("cmd:\"") {
        let rest = &input[start + 5..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].replace("\\\\", "\\").replace("\\\"", "\""));
        }
    }
    None
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
