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

lazy_static::lazy_static! {
    static ref GLOBAL_AI_SETTINGS: Mutex<AiQuotaSettings> = Mutex::new(AiQuotaSettings::default());
    static ref LATEST_PAYLOAD: Mutex<Option<AiQuotaPayload>> = Mutex::new(None);
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
        return "Sẵn sàng (Đã reset)".to_string();
    }
    let diff = target_timestamp - now;
    let hours = diff / 3600;
    let minutes = (diff % 3600) / 60;
    if hours > 24 {
        let days = hours / 24;
        let rem_hours = hours % 24;
        format!("{} ngày {} giờ", days, rem_hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{} phút", minutes)
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

/// Fetch Quota của OpenAI Codex
async fn fetch_codex_quota() -> ProviderQuota {
    let now = get_now_unix();
    let auth_path = match get_codex_auth_path() {
        Some(p) => p,
        None => {
            return ProviderQuota {
                provider: "codex".to_string(),
                connected: false,
                error_message: Some("Không tìm thấy file ~/.codex/auth.json (chưa đăng nhập Codex CLI)".to_string()),
                account_email: None,
                plan_type: None,
                five_hour: None,
                weekly: None,
                secondary_quota: None,
                secondary_label: None,
                last_updated_unix: now,
            };
        }
    };

    let content = match tokio::fs::read_to_string(&auth_path).await {
        Ok(c) => c,
        Err(e) => {
            return ProviderQuota {
                provider: "codex".to_string(),
                connected: false,
                error_message: Some(format!("Không thể đọc auth.json: {}", e)),
                account_email: None,
                plan_type: None,
                five_hour: None,
                weekly: None,
                secondary_quota: None,
                secondary_label: None,
                last_updated_unix: now,
            };
        }
    };

    let json_val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return ProviderQuota {
                provider: "codex".to_string(),
                connected: false,
                error_message: Some(format!("Lỗi parse JSON auth.json: {}", e)),
                account_email: None,
                plan_type: None,
                five_hour: None,
                weekly: None,
                secondary_quota: None,
                secondary_label: None,
                last_updated_unix: now,
            };
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
            return ProviderQuota {
                provider: "codex".to_string(),
                connected: false,
                error_message: Some("Không tìm thấy access_token trong auth.json".to_string()),
                account_email: None,
                plan_type: None,
                five_hour: None,
                weekly: None,
                secondary_quota: None,
                secondary_label: None,
                last_updated_unix: now,
            };
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

                    // Parse five_hour_limit
                    if let Some(fh) = res_json.pointer("/usage/five_hour_limit").or_else(|| res_json.get("five_hour_limit")) {
                        let pct = fh.get("percent_remaining").and_then(|v| v.as_f64()).unwrap_or(100.0);
                        let resets_at = fh.get("resets_at").and_then(|v| v.as_i64());
                        five_hour = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    }

                    // Parse weekly_limit
                    if let Some(wl) = res_json.pointer("/usage/weekly_limit").or_else(|| res_json.get("weekly_limit")) {
                        let pct = wl.get("percent_remaining").and_then(|v| v.as_f64()).unwrap_or(100.0);
                        let resets_at = wl.get("resets_at").and_then(|v| v.as_i64());
                        weekly = Some(QuotaWindow {
                            percent_remaining: pct,
                            reset_time_desc: resets_at.map(format_duration_desc),
                            reset_timestamp: resets_at,
                        });
                    }

                    // Fallback nếu JSON trả về dạng trực tiếp khác
                    if five_hour.is_none() {
                        if let Some(rem) = res_json.pointer("/five_hour_percent").and_then(|v| v.as_f64()) {
                            five_hour = Some(QuotaWindow {
                                percent_remaining: rem,
                                reset_time_desc: None,
                                reset_timestamp: None,
                            });
                        }
                    }

                    let plan = res_json
                        .get("plan_type")
                        .or_else(|| res_json.pointer("/account/plan_type"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return ProviderQuota {
                        provider: "codex".to_string(),
                        connected: true,
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
                }
            } else if status.as_u16() == 401 {
                return ProviderQuota {
                    provider: "codex".to_string(),
                    connected: false,
                    error_message: Some("Token Codex đã hết hạn. Hãy chạy 'codex login' để làm mới".to_string()),
                    account_email,
                    plan_type: None,
                    five_hour: None,
                    weekly: None,
                    secondary_quota: None,
                    secondary_label: None,
                    last_updated_unix: now,
                };
            }

            ProviderQuota {
                provider: "codex".to_string(),
                connected: false,
                error_message: Some(format!("Máy chủ Codex phản hồi mã HTTP {}", status)),
                account_email,
                plan_type: None,
                five_hour: None,
                weekly: None,
                secondary_quota: None,
                secondary_label: None,
                last_updated_unix: now,
            }
        }
        Err(e) => ProviderQuota {
            provider: "codex".to_string(),
            connected: false,
            error_message: Some(format!("Lỗi kết nối máy chủ Codex: {}", e)),
            account_email,
            plan_type: None,
            five_hour: None,
            weekly: None,
            secondary_quota: None,
            secondary_label: None,
            last_updated_unix: now,
        },
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

#[cfg(not(target_os = "windows"))]
fn check_active_window_is_ide() -> (bool, Option<String>) {
    (false, None)
}

/// Fetch toàn bộ dữ liệu Quota theo cấu hình
pub async fn fetch_all_quota_data() -> AiQuotaPayload {
    let settings = { GLOBAL_AI_SETTINGS.lock().unwrap().clone() };
    let mut sys = System::new();

    let codex_quota = if settings.show_codex {
        Some(fetch_codex_quota().await)
    } else {
        None
    };

    let antigravity_quota = if settings.show_antigravity {
        Some(fetch_antigravity_quota(&mut sys).await)
    } else {
        None
    };

    let (active_is_ide, active_app) = check_active_window_is_ide();
    let now = get_now_unix();

    let payload = AiQuotaPayload {
        codex: codex_quota,
        antigravity: antigravity_quota,
        active_window_is_ide: active_is_ide,
        active_app_name: active_app,
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

        loop {
            tokio::time::sleep(Duration::from_millis(1000)).await;

            let settings = { GLOBAL_AI_SETTINGS.lock().unwrap().clone() };
            if !settings.enabled {
                continue;
            }

            let (is_ide, app_name) = check_active_window_is_ide();
            let interval = Duration::from_secs(settings.refresh_interval_sec.max(15));
            let should_poll = last_poll.elapsed() >= interval || (is_ide && !was_ide_active);

            if should_poll {
                last_poll = Instant::now();
                let payload = fetch_all_quota_data().await;
                let _ = app.emit("ai-quota-event", payload);
            } else if is_ide != was_ide_active {
                // Cập nhật trạng thái active window mà không cần fetch lại toàn bộ HTTP
                if let Some(mut cached) = LATEST_PAYLOAD.lock().unwrap().clone() {
                    cached.active_window_is_ide = is_ide;
                    cached.active_app_name = app_name;
                    let _ = app.emit("ai-quota-event", cached);
                }
            }

            was_ide_active = is_ide;
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
