use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::session_tailer::AgyStreamEvent;
use super::types::{AgyActivityPayload, AgyState};

const COMPLETED_HOLD_DURATION: Duration = Duration::from_millis(2500);
const INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);

pub struct AgyStateAggregator {
    current_payload: AgyActivityPayload,
    state_since_instant: Instant,
    last_event_instant: Instant,
}

impl AgyStateAggregator {
    pub fn new() -> Self {
        let now_inst = Instant::now();
        Self {
            current_payload: AgyActivityPayload::default(),
            state_since_instant: now_inst,
            last_event_instant: now_inst,
        }
    }

    pub fn current_payload(&self) -> &AgyActivityPayload {
        &self.current_payload
    }

    pub fn handle_event(&mut self, event: AgyStreamEvent) -> bool {
        let now_inst = Instant::now();
        let now_unix = current_unix_ms();
        self.last_event_instant = now_inst;

        let prev_state = self.current_payload.state;

        match event {
            AgyStreamEvent::UserInput { step_index, timestamp_ms } => {
                self.current_payload.state = AgyState::Thinking;
                self.current_payload.step_index = step_index;
                self.current_payload.active_tool = None;
                self.current_payload.detail_message = Some("Đang suy nghĩ...".to_string());
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }

            AgyStreamEvent::ThinkingStarted { step_index, thinking_preview, timestamp_ms } => {
                if self.current_payload.state != AgyState::Executing {
                    self.current_payload.state = AgyState::Thinking;
                    self.current_payload.step_index = step_index;
                    self.current_payload.active_tool = None;
                    self.current_payload.detail_message = thinking_preview.or_else(|| Some("Đang phân tích...".to_string()));
                    self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                    self.state_since_instant = now_inst;
                }
            }

            AgyStreamEvent::ToolCallStarted { step_index, tool_name, detail, timestamp_ms } => {
                self.current_payload.state = AgyState::Executing;
                self.current_payload.step_index = step_index;
                self.current_payload.active_tool = Some(tool_name);
                self.current_payload.detail_message = Some(detail);
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }

            AgyStreamEvent::WaitingApproval { step_index, prompt, timestamp_ms } => {
                self.current_payload.state = AgyState::WaitingApproval;
                self.current_payload.step_index = step_index;
                self.current_payload.active_tool = Some("ask_question".to_string());
                self.current_payload.detail_message = prompt.or_else(|| Some("Chờ phản hồi của bạn".to_string()));
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }

            AgyStreamEvent::TurnCompleted { step_index, timestamp_ms } => {
                self.current_payload.state = AgyState::Completed;
                self.current_payload.step_index = step_index;
                self.current_payload.active_tool = None;
                self.current_payload.detail_message = Some("Hoàn tất phản hồi".to_string());
                self.current_payload.since_unix_ms = timestamp_ms.max(now_unix);
                self.state_since_instant = now_inst;
            }
        }

        self.current_payload.state != prev_state
    }

    pub fn tick(&mut self) -> bool {
        let now_inst = Instant::now();

        // 1. Completed state hold timer -> transition to Idle
        if self.current_payload.state == AgyState::Completed {
            if now_inst.duration_since(self.state_since_instant) >= COMPLETED_HOLD_DURATION {
                self.current_payload.state = AgyState::Idle;
                self.current_payload.active_tool = None;
                self.current_payload.detail_message = None;
                self.state_since_instant = now_inst;
                return true;
            }
        }

        // 2. Inactivity timeout
        if matches!(self.current_payload.state, AgyState::Thinking | AgyState::Executing) {
            if now_inst.duration_since(self.last_event_instant) >= INACTIVITY_TIMEOUT {
                self.current_payload.state = AgyState::Idle;
                self.current_payload.active_tool = None;
                self.current_payload.detail_message = None;
                self.state_since_instant = now_inst;
                return true;
            }
        }

        false
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
