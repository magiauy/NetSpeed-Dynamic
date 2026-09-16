use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::System;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub percent_remaining: f64,
    pub reset_time_desc: Option<String>,
    pub reset_timestamp: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderQuota {
    pub provider: String, // "codex" | "antigravity"
    pub connected: bool,
    #[serde(default)]
    pub is_fallback: Option<bool>,
    #[serde(default)]
    pub retry_in_sec: Option<u64>,
    #[serde(default)]
    pub is_active: Option<bool>,
    pub error_message: Option<String>,
    pub account_email: Option<String>,
    pub plan_type: Option<String>,
    pub five_hour: Option<QuotaWindow>,
    pub weekly: Option<QuotaWindow>,
    pub secondary_quota: Option<QuotaWindow>, // e.g. Claude or Gemini Ultra
    pub secondary_label: Option<String>,
    pub last_updated_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiQuotaPayload {
    pub codex: Option<ProviderQuota>,
    pub antigravity: Option<ProviderQuota>,
    pub active_window_is_ide: bool,
    pub ide_is_open: bool,
    pub active_app_name: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiQuotaSettings {
    pub enabled: bool,
    pub show_codex: bool,
    pub show_antigravity: bool,
    pub display_mode: String, // "auto" | "always"
    pub refresh_interval_sec: u64,
}

impl Default for AiQuotaSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            show_codex: true,
            show_antigravity: true,
            display_mode: "auto".to_string(),
            refresh_interval_sec: 60,
        }
    }
}

#[derive(Debug, Clone)]
struct CodexStateTracker {
    last_valid_quota: Option<ProviderQuota>,
    consecutive_errors: u32,
    next_retry_time: Option<Instant>,
    last_backoff_secs: u64,
}

impl Default for CodexStateTracker {
    fn default() -> Self {
        Self {
            last_valid_quota: None,
            consecutive_errors: 0,
            next_retry_time: None,
            last_backoff_secs: 0,
        }
    }
}

lazy_static::lazy_static! {
    static ref GLOBAL_AI_SETTINGS: Mutex<AiQuotaSettings> = Mutex::new(AiQuotaSettings::default());
    static ref LATEST_PAYLOAD: Mutex<Option<AiQuotaPayload>> = Mutex::new(None);
    static ref CODEX_TRACKER: Mutex<CodexStateTracker> = Mutex::new(CodexStateTracker::default());
}

/// Tính thời gian retry lũy tiến (5s -> 10s -> 20s -> 40s -> 80s -> tối đa 120s)
fn calculate_backoff_secs(consecutive_errors: u32) -> u64 {
    let base = 5u64;
    let shift = consecutive_errors.saturating_sub(1).min(5);
    (base * (1u64 << shift)).min(120)
}

fn record_codex_success(quota: &ProviderQuota) {
    if let Ok(mut tracker) = CODEX_TRACKER.lock() {
        tracker.last_valid_quota = Some(quota.clone());
        tracker.consecutive_errors = 0;
        tracker.next_retry_time = None;
        tracker.last_backoff_secs = 0;
    }
}

fn record_codex_failure(error_msg: String) -> ProviderQuota {
    let mut tracker = match CODEX_TRACKER.lock() {
        Ok(t) => t,
        Err(poisoned) => poisoned.into_inner(),
    };

    tracker.consecutive_errors += 1;
    let backoff_secs = calculate_backoff_secs(tracker.consecutive_errors);
    tracker.next_retry_time = Some(Instant::now() + Duration::from_secs(backoff_secs));
    tracker.last_backoff_secs = backoff_secs;

    let now = get_now_unix();
    if let Some(mut cached) = tracker.last_valid_quota.clone() {
        cached.connected = false;
        cached.is_fallback = Some(true);
        cached.retry_in_sec = Some(backoff_secs);
        cached.is_active = Some(false);
        cached.error_message = Some(format!(
            "{} (Đang dùng dữ liệu cũ, thử lại sau {}s - lần {})",
            error_msg, backoff_secs, tracker.consecutive_errors
        ));
        cached.last_updated_unix = now;
        cached
    } else {
        ProviderQuota {
            provider: "codex".to_string(),
            connected: false,
            is_fallback: Some(false),
            retry_in_sec: Some(backoff_secs),
            is_active: Some(false),
            error_message: Some(format!(
                "{} (Thử lại sau {}s - lần {})",
                error_msg, backoff_secs, tracker.consecutive_errors
            )),
            account_email: None,
            plan_type: None,
            five_hour: None,
            weekly: None,
            secondary_quota: None,
            secondary_label: None,
            last_updated_unix: now,
        }
    }
}

fn should_retry_codex() -> bool {
    if let Ok(tracker) = CODEX_TRACKER.lock() {
        if tracker.consecutive_errors > 0 {
            if let Some(next_time) = tracker.next_retry_time {
                return Instant::now() >= next_time;
            }
        }
    }
    false
}

fn get_now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn format_duration_desc(target_timestamp: i64) -> String {
    let now = get_now_unix();
    if target_timestamp <= now {
        return "Đã reset".to_string();
    }
    let diff = target_timestamp - now;
    let hours = diff / 3600;
    let minutes = (diff % 3600) / 60;
    if hours > 24 {
        let days = hours / 24;
        let rem_hours = hours % 24;
        if rem_hours > 0 {
            format!("{}d {}h", days, rem_hours)
        } else {
            format!("{}d", days)
        }
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes.max(1))
    }
}

/// Lấy đường dẫn file auth.json của Codex trên máy
fn get_codex_auth_path() -> Option<PathBuf> {
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let path = PathBuf::from(profile).join(".codex").join("auth.json");
        if path.exists() {
            return Some(path);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(".codex").join("auth.json");
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Kiểm tra xem Codex có đang thực hiện tác vụ / suy nghĩ / sinh code không
/// Ưu tiên 1: Lấy trực tiếp từ bộ phát hiện ngữ nghĩa thời gian thực (codex_detector)
/// Ưu tiên 2 (Fallback): Quét file sửa đổi gần nhất trong ~/.codex nếu bộ detector chưa bắt được session
pub fn check_codex_is_working() -> bool {
    if crate::codex_detector::is_codex_active() {
        return true;
    }

    let base_dirs = [
        std::env::var("USERPROFILE").ok().map(|p| PathBuf::from(p).join(".codex")),
        std::env::var("HOME").ok().map(|p| PathBuf::from(p).join(".codex")),
    ];

    let now = std::time::SystemTime::now();

    for dir in base_dirs.iter().flatten() {
        if !dir.exists() {
            continue;
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_lowercase();
                if file_name.ends_with(".sqlite-wal")
                    || file_name.ends_with(".jsonl")
                    || file_name == ".codex-global-state.json"
                {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if let Ok(elapsed) = now.duration_since(modified) {
                                if elapsed <= Duration::from_millis(3500) {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// Fetch Quota của OpenAI Codex
async fn fetch_codex_quota() -> ProviderQuota {
    let now = get_now_unix();
    let auth_path = match get_codex_auth_path() {
        Some(p) => p,
        None => {
            return record_codex_failure("Không tìm thấy file ~/.codex/auth.json (chưa đăng nhập Codex CLI)".to_string());
        }
    };

    let content = match tokio::fs::read_to_string(&auth_path).await {
        Ok(c) => c,
        Err(e) => {
            return record_codex_failure(format!("Không thể đọc auth.json: {}", e));
        }
    };

    let json_val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return record_codex_failure(format!("Lỗi parse JSON auth.json: {}", e));
        }
    };

    // Tìm access token
    let token = json_val
        .pointer("/tokens/access_token")
        .and_then(|v| v.as_str())
        .or_else(|| json_val.get("access_token").and_then(|v| v.as_str()))
        .or_else(|| json_val.get("auth_token").and_then(|v| v.as_str()))
        .or_else(|| json_val.get("token").and_then(|v| v.as_str()));

    let token_str = match token {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return record_codex_failure("Không tìm thấy access_token trong auth.json".to_string());
        }
    };

    let account_email = json_val
        .pointer("/tokens/account_id")
        .or_else(|| json_val.get("email"))
        .or_else(|| json_val.get("account_email"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default();

    let resp = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {}", token_str))
        .header("User-Agent", "NetSpeed-Dynamic/1.0 (Windows NT 10.0; Win64; x64)")
        .header("Accept", "application/json")
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                if let Ok(res_json) = r.json::<serde_json::Value>().await {
                    let mut five_hour = None;
                    let mut weekly = None;

                    // 1. Parse rate_limit.primary_window (5h limit: 18000s)
                    if let Some(primary) = res_json.pointer("/rate_limit/primary_window") {
                        let used = primary.get("used_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let pct = (100.0 - used).max(0.0).min(100.0);
                        let resets_at = primary.get("reset_at").and_then(|v| v.as_i64());
                        five_hour = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    } else if let Some(fh) = res_json.pointer("/usage/five_hour_limit").or_else(|| res_json.get("five_hour_limit")) {
                        let pct = fh.get("percent_remaining").and_then(|v| v.as_f64()).unwrap_or(100.0);
                        let resets_at = fh.get("resets_at").and_then(|v| v.as_i64());
                        five_hour = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    }

                    // 2. Parse rate_limit.secondary_window (Weekly limit: 604800s)
                    if let Some(secondary) = res_json.pointer("/rate_limit/secondary_window") {
                        let used = secondary.get("used_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let pct = (100.0 - used).max(0.0).min(100.0);
                        let resets_at = secondary.get("reset_at").and_then(|v| v.as_i64());
                        weekly = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    } else if let Some(wl) = res_json.pointer("/usage/weekly_limit").or_else(|| res_json.get("weekly_limit")) {
                        let pct = wl.get("percent_remaining").and_then(|v| v.as_f64()).unwrap_or(100.0);
                        let resets_at = wl.get("resets_at").and_then(|v| v.as_i64());
                        weekly = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    }

                    let account_email = res_json
                        .get("email")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .or(account_email);

                    let plan = res_json
                        .get("plan_type")
                        .or_else(|| res_json.pointer("/account/plan_type"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let quota = ProviderQuota {
                        provider: "codex".to_string(),
                        connected: true,
                        is_fallback: Some(false),
                        retry_in_sec: None,
                        is_active: Some(check_codex_is_working()),
                        error_message: None,
                        account_email,
                        plan_type: plan.or_else(|| Some("ChatGPT / Codex".to_string())),
                        five_hour: five_hour.or_else(|| Some(QuotaWindow {
                            percent_remaining: 100.0,
                            reset_time_desc: Some("Đầy hạn mức".to_string()),
                            reset_timestamp: None,
                        })),
                        weekly,
                        secondary_quota: None,
                        secondary_label: None,
                        last_updated_unix: now,
                    };

                    record_codex_success(&quota);
                    return quota;
                }
            } else if status.as_u16() == 401 {
                return record_codex_failure("Token Codex đã hết hạn. Hãy chạy 'codex login' để làm mới".to_string());
            }

            record_codex_failure(format!("Máy chủ Codex phản hồi mã HTTP {}", status))
        }
        Err(e) => {
            record_codex_failure(format!("Lỗi kết nối máy chủ Codex: {}", e))
        }
    }
}

/// Tìm binary agy.exe tuyệt đối trên máy (ưu tiên Local AppData, PATH)
fn find_agy_executable() -> Option<PathBuf> {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(&local_app_data).join("agy").join("bin").join("agy.exe");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(user_profile) = std::env::var("USERPROFILE") {
        let p = PathBuf::from(&user_profile).join("AppData").join("Local").join("agy").join("bin").join("agy.exe");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join("agy.exe");
            if candidate.exists() {
                return Some(candidate);
            }
            #[cfg(not(target_os = "windows"))]
            {
                let candidate = dir.join("agy");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Chuyển đổi chuỗi ISO8601 (e.g. "2026-09-16T18:03:49Z") thành unix timestamp
fn parse_iso8601_to_unix(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.timestamp())
}

/// Fetch Quota trực tiếp qua agy CLI (`agy -p "/usage" --output-format json`) chạy ngầm 100% không flash console
async fn fetch_antigravity_via_cli() -> Option<ProviderQuota> {
    let now = get_now_unix();
    let agy_bin = find_agy_executable()?;

    let mut cmd = tokio::process::Command::new(agy_bin);
    cmd.args(["-p", "/usage", "--output-format", "json"]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
    
    #[cfg(target_os = "windows")]
    {
        // 0x08000000: CREATE_NO_WINDOW (Không tạo cửa sổ console)
        // 0x00000200: CREATE_NEW_PROCESS_GROUP (Tạo process group riêng, tránh gắn vào console cha)
        cmd.creation_flags(0x08000000 | 0x00000200);
    }

    let output = match tokio::time::timeout(Duration::from_secs(8), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => out,
        _ => return None,
    };

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let res_json: serde_json::Value = serde_json::from_str(stdout_str.trim()).ok()?;

    if res_json.get("status").and_then(|s| s.as_str()) != Some("SUCCESS") {
        return None;
    }

    let groups = res_json.pointer("/command/data/groups").and_then(|v| v.as_array())?;

    let mut gemini_5h: Option<QuotaWindow> = None;
    let mut gemini_weekly: Option<QuotaWindow> = None;
    let mut claude_5h: Option<QuotaWindow> = None;
    let mut claude_weekly: Option<QuotaWindow> = None;

    for group in groups {
        let group_name = group.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_lowercase();
        let buckets = group.get("buckets").and_then(|v| v.as_array());

        if let Some(b_list) = buckets {
            for b in b_list {
                let window = b.get("window").and_then(|v| v.as_str()).unwrap_or_default();
                let remaining_fraction = b.get("remaining_fraction").and_then(|v| v.as_f64()).unwrap_or(1.0);
                let pct = (remaining_fraction * 100.0).round().max(0.0).min(100.0);
                let reset_iso = b.get("reset_time").and_then(|v| v.as_str());
                let reset_ts = reset_iso.and_then(parse_iso8601_to_unix);
                let reset_desc = reset_ts.map(format_duration_desc);

                let quota_win = QuotaWindow {
                    percent_remaining: pct,
                    reset_time_desc: reset_desc,
                    reset_timestamp: reset_ts,
                };

                if group_name.contains("gemini") {
                    if window == "5h" {
                        gemini_5h = Some(quota_win);
                    } else if window == "weekly" {
                        gemini_weekly = Some(quota_win);
                    }
                } else if group_name.contains("claude") || group_name.contains("gpt") || group_name.contains("3p") {
                    if window == "5h" {
                        claude_5h = Some(quota_win);
                    } else if window == "weekly" {
                        claude_weekly = Some(quota_win);
                    }
                }
            }
        }
    }

    if gemini_5h.is_some() || gemini_weekly.is_some() || claude_5h.is_some() {
        Some(ProviderQuota {
            provider: "antigravity".to_string(),
            connected: true,
            is_fallback: Some(false),
            retry_in_sec: None,
            is_active: None,
            error_message: None,
            account_email: None,
            plan_type: Some("Google Antigravity".to_string()),
            five_hour: gemini_5h.or_else(|| gemini_weekly.clone()),
            weekly: gemini_weekly,
            secondary_quota: claude_5h.or_else(|| claude_weekly.clone()),
            secondary_label: Some("Claude / GPT".to_string()),
            last_updated_unix: now,
        })
    } else {
        None
    }
}

/// Tìm kiếm Language Server của Google Antigravity đang chạy
fn find_antigravity_language_server(sys: &mut System) -> Option<(u16, Option<String>)> {
    sys.refresh_processes();
    
    // Tìm các tiến trình có tên chứa antigravity, language_server
    for (_pid, proc) in sys.processes() {
        let name = proc.name().to_lowercase();
        if name.contains("antigravity") || name.contains("language_server") || name.contains("agy") {
            let cmd = proc.cmd();
            let mut port: Option<u16> = None;
            let mut csrf: Option<String> = None;

            for (i, arg) in cmd.iter().enumerate() {
                if arg == "--port" || arg == "-p" {
                    if let Some(next) = cmd.get(i + 1) {
                        port = next.parse::<u16>().ok();
                    }
                } else if arg.starts_with("--port=") {
                    port = arg.trim_start_matches("--port=").parse::<u16>().ok();
                } else if arg == "--csrf_token" || arg == "--csrf-token" {
                    if let Some(next) = cmd.get(i + 1) {
                        csrf = Some(next.clone());
                    }
                } else if arg.starts_with("--csrf_token=") {
                    csrf = Some(arg.trim_start_matches("--csrf_token=").to_string());
                } else if arg.starts_with("--csrf-token=") {
                    csrf = Some(arg.trim_start_matches("--csrf-token=").to_string());
                }
            }

            if let Some(p) = port {
                return Some((p, csrf));
            }
        }
    }
    None
}

/// Fetch Quota của Google Antigravity
async fn fetch_antigravity_quota(sys: &mut System) -> ProviderQuota {
    // 1. Thử lấy qua CLI `agy -p "/usage" --output-format json` trước
    if let Some(quota) = fetch_antigravity_via_cli().await {
        return quota;
    }

    // 2. Nếu CLI không khả dụng, fallback sang quét cổng Language Server cục bộ
    let now = get_now_unix();
    let server_info = find_antigravity_language_server(sys);

    if let Some((port, csrf_token)) = server_info {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap_or_default();

        let url = format!("http://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary", port);
        let mut req = client.post(&url).header("Content-Type", "application/json");

        if let Some(csrf) = &csrf_token {
            req = req.header("X-Codeium-Csrf-Token", csrf);
        }

        let resp = req.json(&serde_json::json!({})).send().await;
        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(res_json) = r.json::<serde_json::Value>().await {
                    let mut gemini_quota = None;
                    let mut claude_quota = None;

                    // Parse Gemini model quotas
                    if let Some(gemini) = res_json.pointer("/user_quotas/gemini").or_else(|| res_json.get("gemini_quota")) {
                        let pct = gemini.get("remaining_fraction").and_then(|v| v.as_f64()).map(|f| f * 100.0)
                            .or_else(|| gemini.get("percent_remaining").and_then(|v| v.as_f64()))
                            .unwrap_or(100.0);
                        let resets_at = gemini.get("resets_at").and_then(|v| v.as_i64());
                        gemini_quota = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    }

                    // Parse Claude/Secondary quotas
                    if let Some(claude) = res_json.pointer("/user_quotas/claude").or_else(|| res_json.get("claude_quota")) {
                        let pct = claude.get("remaining_fraction").and_then(|v| v.as_f64()).map(|f| f * 100.0)
                            .or_else(|| claude.get("percent_remaining").and_then(|v| v.as_f64()))
                            .unwrap_or(100.0);
                        let resets_at = claude.get("resets_at").and_then(|v| v.as_i64());
                        claude_quota = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    }

                    return ProviderQuota {
                        provider: "antigravity".to_string(),
                        connected: true,
                        is_fallback: Some(false),
                        retry_in_sec: None,
                        is_active: None,
                        error_message: None,
                        account_email: res_json.get("user_email").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        plan_type: Some("Antigravity Pro".to_string()),
                        five_hour: gemini_quota.or_else(|| Some(QuotaWindow {
                            percent_remaining: 100.0,
                            reset_time_desc: Some("Sẵn sàng".to_string()),
                            reset_timestamp: None,
                        })),
                        weekly: None,
                        secondary_quota: claude_quota,
                        secondary_label: Some("Claude / Sonnet".to_string()),
                        last_updated_unix: now,
                    };
                }
            }
        }
    }

    // Nếu Antigravity đang hoạt động trên máy hoặc có config local
    ProviderQuota {
        provider: "antigravity".to_string(),
        connected: false,
        is_fallback: Some(false),
        retry_in_sec: None,
        is_active: None,
        error_message: None,
        account_email: None,
        plan_type: None,
        five_hour: None,
        weekly: None,
        secondary_quota: None,
        secondary_label: None,
        last_updated_unix: now,
    }
}

/// Kiểm tra xem cửa sổ Foreground (active) hiện tại có phải là VS Code / Codex / IDE không
#[cfg(target_os = "windows")]
fn check_active_window_is_ide() -> (bool, Option<String>) {
    use winapi::shared::minwindef::{FALSE, TRUE};
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::OpenProcess;
    use winapi::um::winbase::QueryFullProcessImageNameW;
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
    use winapi::um::winuser::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return (false, None);
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return (false, None);
        }

        let mut process_name = String::new();
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if !process.is_null() {
            let mut path_buf = [0u16; 1024];
            let mut path_len = path_buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, 0, path_buf.as_mut_ptr(), &mut path_len);
            CloseHandle(process);
            if ok == TRUE {
                process_name = String::from_utf16_lossy(&path_buf[..path_len as usize]).to_lowercase();
            }
        }

        let mut title_buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
        let title = if n > 0 {
            String::from_utf16_lossy(&title_buf[..n as usize]).to_lowercase()
        } else {
            String::new()
        };

        let is_ide = process_name.ends_with("code.exe")
            || process_name.ends_with("cursor.exe")
            || process_name.ends_with("windsurf.exe")
            || process_name.ends_with("codex.exe")
            || process_name.ends_with("vscodium.exe")
            || process_name.ends_with("antigravity.exe")
            || process_name.ends_with("devenv.exe")
            || title.contains("visual studio code")
            || title.contains("cursor")
            || title.contains("windsurf")
            || title.contains("antigravity")
            || title.contains("codex");

        let app_name = if process_name.ends_with("code.exe") {
            Some("VS Code".to_string())
        } else if process_name.ends_with("cursor.exe") {
            Some("Cursor".to_string())
        } else if process_name.ends_with("windsurf.exe") {
            Some("Windsurf".to_string())
        } else if process_name.ends_with("codex.exe") {
            Some("Codex".to_string())
        } else if process_name.ends_with("antigravity.exe") {
            Some("Antigravity".to_string())
        } else {
            None
        };

        (is_ide, app_name)
    }
}

/// Kiểm tra xem có bất kỳ cửa sổ / tiến trình VS Code / Codex / Antigravity nào đang mở trên hệ thống không
#[cfg(target_os = "windows")]
fn check_ide_is_open() -> (bool, Option<String>) {
    use winapi::shared::minwindef::{BOOL, LPARAM, TRUE};
    use winapi::shared::windef::HWND;
    use winapi::um::winuser::{EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible};

    struct WindowSearchData {
        found: bool,
        app_name: Option<String>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam as *mut WindowSearchData);
        if IsWindowVisible(hwnd) != 0 {
            let len = GetWindowTextLengthW(hwnd);
            if len > 0 && len < 1024 {
                let mut buf = vec![0u16; (len + 1) as usize];
                let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), (len + 1) as i32);
                if n > 0 {
                    let title = String::from_utf16_lossy(&buf[..n as usize]).to_lowercase();
                    if title.contains("visual studio code") || title.ends_with(" - code") || title == "code" {
                        data.found = true;
                        data.app_name = Some("VS Code".to_string());
                        return 0; // stop enum
                    } else if title.contains("codex") {
                        data.found = true;
                        data.app_name = Some("Codex".to_string());
                        return 0;
                    } else if title.contains("cursor") {
                        data.found = true;
                        data.app_name = Some("Cursor".to_string());
                        return 0;
                    } else if title.contains("antigravity") {
                        data.found = true;
                        data.app_name = Some("Antigravity".to_string());
                        return 0;
                    } else if title.contains("windsurf") {
                        data.found = true;
                        data.app_name = Some("Windsurf".to_string());
                        return 0;
                    }
                }
            }
        }
        TRUE
    }

    let mut data = WindowSearchData {
        found: false,
        app_name: None,
    };

    unsafe {
        EnumWindows(Some(enum_proc), &mut data as *mut _ as LPARAM);
    }

    if data.found {
        (true, data.app_name)
    } else {
        check_active_window_is_ide()
    }
}

#[cfg(not(target_os = "windows"))]
fn check_active_window_is_ide() -> (bool, Option<String>) {
    (false, None)
}

#[cfg(not(target_os = "windows"))]
fn check_ide_is_open() -> (bool, Option<String>) {
    (false, None)
}

/// Fetch toàn bộ dữ liệu Quota theo cấu hình
pub async fn fetch_all_quota_data() -> AiQuotaPayload {
    let settings = { GLOBAL_AI_SETTINGS.lock().unwrap().clone() };

    let codex_fut = async {
        if settings.show_codex {
            Some(fetch_codex_quota().await)
        } else {
            None
        }
    };

    let antigravity_fut = async {
        if settings.show_antigravity {
            let mut sys = System::new();
            Some(fetch_antigravity_quota(&mut sys).await)
        } else {
            None
        }
    };

    let (codex_quota, antigravity_quota) = tokio::join!(codex_fut, antigravity_fut);

    let (active_is_ide, active_app) = check_active_window_is_ide();
    let (ide_open, open_app) = check_ide_is_open();
    let now = get_now_unix();

    let payload = AiQuotaPayload {
        codex: codex_quota,
        antigravity: antigravity_quota,
        active_window_is_ide: active_is_ide,
        ide_is_open: ide_open,
        active_app_name: active_app.or(open_app),
        timestamp: now,
    };

    {
        let mut latest = LATEST_PAYLOAD.lock().unwrap();
        *latest = Some(payload.clone());
    }

    payload
}

/// Khởi chạy Background Monitor cho AI Quota
pub fn start_ai_quota_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_poll = Instant::now() - Duration::from_secs(300);
        let mut was_ide_active = false;
        let mut was_ide_open = false;
        let mut was_codex_active = false;

        loop {
            tokio::time::sleep(Duration::from_millis(800)).await;

            let settings = { GLOBAL_AI_SETTINGS.lock().unwrap().clone() };
            if !settings.enabled {
                continue;
            }

            let (is_ide, app_name) = check_active_window_is_ide();
            let (is_open, open_app_name) = check_ide_is_open();
            let is_codex_active = if settings.show_codex {
                check_codex_is_working()
            } else {
                false
            };

            let is_match = is_ide || is_open;
            let interval = Duration::from_secs(settings.refresh_interval_sec.max(15));
            let codex_retry_due = settings.show_codex && should_retry_codex();
            let should_poll = codex_retry_due
                || last_poll.elapsed() >= interval
                || (is_match && (!was_ide_active && !was_ide_open));

            let codex_active_changed = is_codex_active != was_codex_active;
            let window_state_changed = is_ide != was_ide_active || is_open != was_ide_open;

            if should_poll {
                last_poll = Instant::now();
                let mut payload = fetch_all_quota_data().await;
                if let Some(ref mut c) = payload.codex {
                    c.is_active = Some(is_codex_active);
                }
                let _ = app.emit("ai-quota-event", payload);
            } else if window_state_changed || codex_active_changed {
                // Cập nhật trạng thái active/open window hoặc trạng thái Codex working mà không cần fetch lại toàn bộ HTTP
                if let Some(mut cached) = LATEST_PAYLOAD.lock().unwrap().clone() {
                    cached.active_window_is_ide = is_ide;
                    cached.ide_is_open = is_open;
                    cached.active_app_name = app_name.or(open_app_name);
                    if let Some(ref mut c) = cached.codex {
                        c.is_active = Some(is_codex_active);
                    }
                    let _ = app.emit("ai-quota-event", cached.clone());
                    *LATEST_PAYLOAD.lock().unwrap() = Some(cached);
                }
            }

            was_ide_active = is_ide;
            was_ide_open = is_open;
            was_codex_active = is_codex_active;
        }
    });
}

// -------------------------------------------------------------
// Tauri Commands
// -------------------------------------------------------------

#[tauri::command]
pub async fn get_ai_quota_data() -> Result<AiQuotaPayload, String> {
    let cached = {
        LATEST_PAYLOAD.lock().unwrap().clone()
    };
    if let Some(c) = cached {
        Ok(c)
    } else {
        Ok(fetch_all_quota_data().await)
    }
}

#[tauri::command]
pub async fn refresh_ai_quota(app: AppHandle) -> Result<AiQuotaPayload, String> {
    let payload = fetch_all_quota_data().await;
    let _ = app.emit("ai-quota-event", payload.clone());
    Ok(payload)
}

#[tauri::command]
pub fn get_ai_quota_settings() -> Result<AiQuotaSettings, String> {
    Ok(GLOBAL_AI_SETTINGS.lock().unwrap().clone())
}

#[tauri::command]
pub fn save_ai_quota_settings(settings: AiQuotaSettings) -> Result<(), String> {
    let mut s = GLOBAL_AI_SETTINGS.lock().unwrap();
    *s = settings;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_antigravity() {
        let quota = fetch_antigravity_via_cli().await;
        println!("CLI Result: {:?}", quota);
        let mut sys = System::new();
        let full_quota = fetch_antigravity_quota(&mut sys).await;
        println!("Full Antigravity Quota: {:?}", full_quota);
        assert!(full_quota.connected);
    }

    #[tokio::test]
    async fn test_fetch_codex_raw() {
        let quota = fetch_codex_quota().await;
        println!("Codex ProviderQuota: {:#?}", quota);

        // Raw request test
        if let Some(auth_path) = get_codex_auth_path() {
            if let Ok(c) = tokio::fs::read_to_string(&auth_path).await {
                if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&c) {
                    let token = json_val
                        .pointer("/tokens/access_token")
                        .and_then(|v| v.as_str())
                        .or_else(|| json_val.get("access_token").and_then(|v| v.as_str()));
                    if let Some(token_str) = token {
                        let client = reqwest::Client::new();
                        let resp = client
                            .get("https://chatgpt.com/backend-api/wham/usage")
                            .header("Authorization", format!("Bearer {}", token_str))
                            .header("User-Agent", "NetSpeed-Dynamic/1.0")
                            .send()
                            .await;
                        println!("HTTP response: {:?}", resp);
                        if let Ok(r) = resp {
                            let text = r.text().await;
                            println!("Raw body: {:?}", text);
                        }
                    }
                }
            }
        }
    }
}


