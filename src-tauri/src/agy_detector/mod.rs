pub mod aggregator;
pub mod session_tailer;
pub mod types;

use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use aggregator::AgyStateAggregator;
use session_tailer::AgySessionTailer;
pub use types::{AgyActivityPayload, AgyState};

fn should_refresh_quota_for_transition(previous: AgyState, current: AgyState) -> bool {
    (previous == AgyState::Idle && current != AgyState::Idle)
        || (previous != AgyState::Idle && current == AgyState::Idle)
}

lazy_static::lazy_static! {
    static ref GLOBAL_AGY_ACTIVITY: Mutex<AgyActivityPayload> = Mutex::new(AgyActivityPayload::default());
}

/// Returns whether Antigravity is currently actively working (Thinking or Executing)
#[allow(dead_code)]
pub fn is_agy_active() -> bool {
    if let Ok(guard) = GLOBAL_AGY_ACTIVITY.lock() {
        matches!(guard.state, AgyState::Thinking | AgyState::Executing)
    } else {
        false
    }
}

/// Get current full Antigravity activity snapshot
pub fn get_current_activity() -> AgyActivityPayload {
    GLOBAL_AGY_ACTIVITY
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Start background Antigravity semantic activity detector
pub fn start_agy_detector(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut tailer = AgySessionTailer::new();
        let mut aggregator = AgyStateAggregator::new();
        let mut last_emitted_state = AgyState::Idle;
        let mut ticker = tokio::time::interval(Duration::from_millis(100));

        loop {
            ticker.tick().await;

            // 1. Poll for new JSONL entries from active transcript
            tailer.poll_events(&tx);

            // 2. Drain all received events into aggregator
            let mut state_changed = false;
            while let Ok(event) = rx.try_recv() {
                if aggregator.handle_event(event) {
                    state_changed = true;
                }
            }

            // 3. Tick timer checks (hold durations, timeout)
            if aggregator.tick() {
                state_changed = true;
            }

            let current = aggregator.current_payload().clone();

            // 4. Update global memory cache
            if let Ok(mut guard) = GLOBAL_AGY_ACTIVITY.lock() {
                *guard = current.clone();
            }

            // 5. Emit Tauri event when state flips
            if state_changed || current.state != last_emitted_state {
                let prev_state = last_emitted_state;
                last_emitted_state = current.state;
                let _ = app.emit("agy-activity-event", current.clone());

                // Khi AGY hoàn tất turn (Completed) hoặc kết thúc về Idle, kích hoạt đồng bộ Quota % mới ngay lập tức
                if (current.state == AgyState::Completed && prev_state != AgyState::Completed)
                    || should_refresh_quota_for_transition(prev_state, current.state)
                {
                    crate::ai_quota::request_immediate_quota_refresh();
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_quota_when_agy_session_enters_or_leaves_active_state() {
        assert!(should_refresh_quota_for_transition(AgyState::Idle, AgyState::Thinking));
        assert!(should_refresh_quota_for_transition(AgyState::Executing, AgyState::Idle));
        assert!(!should_refresh_quota_for_transition(AgyState::Thinking, AgyState::Executing));
    }
}
