# Thiết Kế Module Quota Codex & Antigravity Cho NetSpeed-Dynamic

## 1. Tổng quan (Overview)
Tính năng mới bổ sung vào NetSpeed-Dynamic cho phép người dùng giám sát hạn ngạch sử dụng (Quota / Rate Limits) của **OpenAI Codex** và **Google Antigravity** theo thời gian thực:
- Hiển thị nhỏ gọn trên **Dynamic Island** (mặc định hiển thị Quota 5 giờ).
- Khi **Hover** vào Island: Mở rộng hiển thị chi tiết (Quota 5h, Quota Tuần / 7 ngày, thời gian đếm ngược Reset, trạng thái tài khoản, nút Refresh nhanh).
- Cung cấp trang cài đặt trong **Console (Main Panel)** để bật/tắt từng nhà cung cấp, chọn chế độ hiển thị (*Luôn hiển thị* hoặc *Tự động khi mở VS Code / Codex*), tùy chỉnh chu kỳ làm mới.

---

## 2. Kiến trúc hệ thống (Architecture)

```
┌────────────────────────────────────────────────────────┐
│               Data Sources (Local Machine)             │
│  - Codex: %USERPROFILE%\.codex\auth.json               │
│  - Antigravity: Local Language Server (127.0.0.1:port) │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼
┌────────────────────────────────────────────────────────┐
│            Rust Backend (src-tauri/src/ai_quota.rs)    │
│  - Auth & Token Loader                                 │
│  - Rate-limit Fetchers (Codex HTTPS & Antigravity LS)  │
│  - Active Window Tracker (Win32 GetForegroundWindow)   │
│  - Background Poller (configurable 30s/60s/120s)       │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼ Tauri Events & Commands
┌────────────────────────────────────────────────────────┐
│                 Frontend Layer (Vue 3)                 │
│  - MainPanel.vue (Console: Config, Toggles, Test)      │
│  - WidgetIsland.vue (Dynamic Island Compact & Popover) │
└────────────────────────────────────────────────────────┘
```

---

## 3. Đặc tả chi tiết các thành phần (Component Specifications)

### 3.1. Backend Rust (`src-tauri/src/ai_quota.rs`)
1. **Codex Quota Fetcher**:
   - Đọc OAuth token từ `%USERPROFILE%\.codex\auth.json`.
   - Gửi request `GET https://chatgpt.com/backend-api/wham/usage` với Bearer token.
   - Phân tích payload:
     - `five_hour_limit`: percentage remaining, reset_time_unix / seconds.
     - `weekly_limit`: percentage remaining, reset_time_unix / seconds.
2. **Antigravity Quota Fetcher**:
   - Quét tiến trình `antigravity` / `language_server` qua `sysinfo` hoặc phát hiện port động và CSRF token (`X-Codeium-Csrf-Token`).
   - Gửi request `POST https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary` (hoặc fallback `GetUserStatus`).
   - Phân tích quota Gemini & Claude.
3. **Window Tracker**:
   - Theo dõi tiêu đề cửa sổ và tên tiến trình (`Code.exe`, `cursor.exe`, `windsurf.exe`, `codex.exe`).
4. **Tauri Commands & Events**:
   - Command `get_ai_quota_data()`: Lấy trạng thái hiện tại.
   - Command `refresh_ai_quota()`: Ép buộc quét lại dữ liệu ngay.
   - Command `save_ai_quota_config(config)`: Lưu cài đặt người dùng.
   - Event `ai-quota-updated`: Phát dữ liệu cập nhật tới UI Widget Island và Main Panel.

---

### 3.2. Console / Cài đặt (`src/views/MainPanel.vue`)
- **Tùy chọn cấu hình**:
  - `enabled`: Bật / Tắt module.
  - `show_codex`: Bật theo dõi OpenAI Codex.
  - `show_antigravity`: Bật theo dõi Google Antigravity.
  - `display_mode`: `'auto'` (chỉ hiện khi focus IDE/Codex) hoặc `'always'` (luôn hiển thị trên Island).
  - `refresh_interval`: 30s, 60s, 120s, 300s.
- **Trực quan hóa trạng thái**:
  - Tình trạng kết nối (Active / Error / Not Logged In).
  - Nút Refresh thủ công.

---

### 3.3. Dynamic Island Widget (`src/views/WidgetIsland.vue`)
- **Trạng thái Compact (Mặc định)**:
  - Hiển thị pill nhỏ gọn chứa Icon (Codex / Antigravity), % hạn ngạch 5h còn lại, vạch màu cảnh báo:
    - 🟢 Xanh lá: `> 30%`
    - 🟡 Vàng: `10% - 30%`
    - 🔴 Đỏ: `< 10%`
- **Trạng thái Hover (Expanded Glass Card)**:
  - Kích thước mở rộng mượt mà (smooth transition).
  - Hiển thị đầy đủ:
    - **5h Window**: % còn lại + thanh tiến trình + thời gian đếm ngược reset (ví dụ: `Reset sau 1h 24m`).
    - **Weekly Window**: % còn lại + thanh tiến trình + thời gian reset tuần.
    - Chế độ hiển thị song song nếu bật cả 2 dịch vụ.
    - Nút thao tác nhanh: Refresh (🔄).

---

## 4. Kiểm thử & Độ tin cậy (Error Handling & Reliability)
- Nếu file `auth.json` không tồn tại hoặc hết hạn: Hiển thị tooltip thân thiện yêu cầu đăng nhập (`codex login`).
- Nếu Language Server của Antigravity chưa khởi động: Giữ trạng thái chờ kết nối, không gây crash ứng dụng.
- Tất cả các request HTTP chạy bất đồng bộ trong background thread, không block UI hoặc ảnh hưởng đến đo tốc độ mạng của NetSpeed-Dynamic.
