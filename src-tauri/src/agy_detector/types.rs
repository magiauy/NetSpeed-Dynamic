use serde::{Deserialize, Serialize};

/// Semantic activity state of Google Antigravity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgyState {
    Idle,
    Thinking,
    Executing,
    WaitingApproval,
    Completed,
    Failed,
}

impl Default for AgyState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Payload emitted to Tauri frontend and consumers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgyActivityPayload {
    pub state: AgyState,
    pub conversation_id: Option<String>,
    pub step_index: Option<u64>,
    pub active_tool: Option<String>,
    pub detail_message: Option<String>,
    pub since_unix_ms: u64,
    pub confidence: f32,
    pub source: String,
}

impl Default for AgyActivityPayload {
    fn default() -> Self {
        Self {
            state: AgyState::Idle,
            conversation_id: None,
            step_index: None,
            active_tool: None,
            detail_message: None,
            since_unix_ms: 0,
            confidence: 1.0,
            source: "agy_transcript".to_string(),
        }
    }
}

/// Raw line parsed from `~/.gemini/antigravity-cli/brain/<conv_id>/.system_generated/logs/transcript.jsonl`
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct RawAgyRecord {
    pub step_index: Option<u64>,
    pub source: Option<String>,
    #[serde(rename = "type")]
    pub record_type: Option<String>,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub content: Option<String>,
    pub thinking: Option<String>,
    pub tool_calls: Option<Vec<RawAgyToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct RawAgyToolCall {
    pub name: Option<String>,
    pub args: Option<serde_json::Value>,
}
