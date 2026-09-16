use serde::{Deserialize, Serialize};

/// Semantic activity state of OpenAI Codex
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexState {
    Idle,
    Thinking,
    Executing,
    WaitingApproval,
    Review,
    Failed,
}

impl Default for CodexState {
    fn default() -> Self {
        Self::Idle
    }
}

/// Specific tool / action kind being executed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    CommandExecution,
    FileEdit,
    McpCall,
    Reasoning,
    WebSearch,
    Other(String),
}

/// Payload emitted to Tauri frontend and consumers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexActivityPayload {
    pub state: CodexState,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub active_tool: Option<ToolKind>,
    pub detail_message: Option<String>,
    pub since_unix_ms: u64,
    pub confidence: f32,
    pub source: String,
}

impl Default for CodexActivityPayload {
    fn default() -> Self {
        Self {
            state: CodexState::Idle,
            session_id: None,
            turn_id: None,
            active_tool: None,
            detail_message: None,
            since_unix_ms: 0,
            confidence: 1.0,
            source: "session_jsonl".to_string(),
        }
    }
}

/// Raw line parsed from `.codex/sessions/**/*.jsonl`
#[derive(Debug, Clone, Deserialize)]
pub struct RawSessionRecord {
    pub timestamp: Option<String>,
    pub ordinal: Option<u64>,
    #[serde(rename = "type")]
    pub record_type: String,
    pub payload: Option<RawPayload>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawPayload {
    #[serde(rename = "type")]
    pub payload_type: Option<String>,
    pub turn_id: Option<String>,
    pub thread_id: Option<String>,
    pub name: Option<String>,
    pub input: Option<String>,
    pub status: Option<String>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}
