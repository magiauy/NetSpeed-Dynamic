pub mod aggregator;
pub mod session_tailer;
pub mod types;

use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use aggregator::StateAggregator;
use session_tailer::SessionTailer;
pub use types::{CodexActivityPayload, CodexState};

lazy_static::lazy_static! {
    static ref GLOBAL_CODEX_ACTIVITY: Mutex<CodexActivityPayload> = Mutex::new(CodexActivityPayload::default());
}

/// Returns whether Codex is currently actively working (Thinking or Executing)
pub fn is_codex_active() -> bool {
    if let Ok(guard) = GLOBAL_CODEX_ACTIVITY.lock() {
        matches!(guard.state, CodexState::Thinking | CodexState::Executing)
    } else {
        false
    }
}

/// Get current full Codex activity snapshot
pub fn get_current_activity() -> CodexActivityPayload {
    GLOBAL_CODEX_ACTIVITY
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Start background Codex semantic activity detector
pub fn start_codex_detector(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut tailer = SessionTailer::new();
        let mut aggregator = StateAggregator::new();
        let mut last_emitted_state = CodexState::Idle;
        let mut ticker = tokio::time::interval(Duration::from_millis(100));

        loop {
            ticker.tick().await;

            // 1. Poll for new JSONL entries from active session files
            tailer.poll_events(&tx);

            // 2. Drain all received events into aggregator
            let mut state_changed = false;
            while let Ok(event) = rx.try_recv() {
                if aggregator.handle_event(event) {
                    state_changed = true;
                }
            }

            // 3. Tick timer checks (gap smoothing, hold durations, timeout)
            if aggregator.tick() {
                state_changed = true;
            }

            let current = aggregator.current_payload().clone();

            // 4. Update global memory cache
            if let Ok(mut guard) = GLOBAL_CODEX_ACTIVITY.lock() {
                *guard = current.clone();
            }

            // 5. Emit Tauri event when state flips
            if state_changed || current.state != last_emitted_state {
                last_emitted_state = current.state;
                let _ = app.emit("codex-activity-event", current.clone());
            }
        }
    });
}
