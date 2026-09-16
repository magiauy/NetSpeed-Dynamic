# AI Quota Monitor (Codex & Antigravity) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Xây dựng module theo dõi hạn ngạch sử dụng (Quota) của OpenAI Codex và Google Antigravity, hiển thị compact (5h quota) trên Dynamic Island, mở rộng chi tiết khi hover, và tích hợp trang cấu hình trong Console (MainPanel).

**Architecture:** 
- Backend Rust (`src-tauri/src/ai_quota.rs`): Chịu trách nhiệm đọc token từ local config, query API hạn ngạch từ OpenAI & Antigravity Language Server, theo dõi active window qua Win32 API, phát sự kiện `ai-quota-event` định kỳ.
- Frontend Vue 3:
  - `src/views/MainPanel.vue`: Thêm giao diện Console quản lý cấu hình, toggle dịch vụ, chế độ hiển thị (*Luôn hiển thị* hoặc *Tự động khi mở VSCode/Codex*).
  - `src/views/WidgetIsland.vue`: Thêm card/pill hiển thị Quota trên Dynamic Island với hiệu ứng hover mở rộng thông tin chi tiết.

**Tech Stack:** Rust (Tauri 2, reqwest, sysinfo, windows-sys), Vue 3 (TypeScript, Tailwind / CSS Scoped, Lucide Icons).

**Spec:** [`docs/superpowers/specs/2026-09-16-ai-quota-monitor-design.md`](file:///D:/IT_K22/NetSpeed-Dynamic/docs/superpowers/specs/2026-09-16-ai-quota-monitor-design.md)

---

## Global Constraints
- Hệ điều hành mục tiêu: Windows 10/11 x64.
- An toàn bảo mật: Đọc token cục bộ không đẩy ra ngoài, request trực tiếp HTTPS tới OpenAI / localhost Antigravity.
- Hiệu năng: Không block luồng UI, xử lý lỗi an toàn (graceful degradation) khi chưa đăng nhập hoặc offline.

---

### Task 1: Xây dựng Backend Module Rust (`src-tauri/src/ai_quota.rs`)

**Files:**
- Create: `src-tauri/src/ai_quota.rs`

**Interfaces:**
- Produces:
  - `AiQuotaPayload`: Struct chứa thông tin quota Codex (5h %, weekly %, reset_time) và Antigravity (Gemini %, Claude %, reset_time), active status.
  - `AiQuotaSettings`: Struct cấu hình (enabled, show_codex, show_antigravity, display_mode, interval_sec).
  - Commands: `get_ai_quota_data`, `refresh_ai_quota`, `save_ai_quota_settings`.
  - Background loop: `start_ai_quota_monitor(app: AppHandle)`.

- [ ] **Step 1: Tạo cấu trúc dữ liệu và parser token/request cho Codex & Antigravity**
- [ ] **Step 2: Triển khai Active Window Tracker (Win32 `GetForegroundWindow` / process check cho Code.exe, codex.exe, cursor.exe)**
- [ ] **Step 3: Triển khai background polling worker với cơ chế `app.emit("ai-quota-event", payload)`**
- [ ] **Step 4: Kiểm tra biên dịch độc lập `cargo check` cho file `ai_quota.rs`**

---

### Task 2: Tích hợp Module vào Tauri Core (`src-tauri/src/lib.rs`)

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `ai_quota::*`
- Produces: Tauri app builder khởi chạy `start_ai_quota_monitor` và đăng ký các commands.

- [ ] **Step 1: Khai báo `mod ai_quota;` trong `src-tauri/src/lib.rs`**
- [ ] **Step 2: Đăng ký các Tauri command (`get_ai_quota_data`, `refresh_ai_quota`, `save_ai_quota_settings`) vào `generate_handler![]`**
- [ ] **Step 3: Khởi động `ai_quota::start_ai_quota_monitor(app.clone())` trong setup lifecycle**
- [ ] **Step 4: Chạy `cargo check` từ thư mục `src-tauri` để đảm bảo không lỗi biên dịch**

---

### Task 3: Tích hợp Cấu hình trong Console (`src/views/MainPanel.vue`)

**Files:**
- Modify: `src/views/MainPanel.vue`
- Modify: `src/i18n.ts` (nếu cần đa ngôn ngữ)

**Interfaces:**
- Consumes: Tauri invoke (`get_ai_quota_data`, `save_ai_quota_settings`, `refresh_ai_quota`).

- [ ] **Step 1: Thêm mục cài đặt "Hạn ngạch AI (Codex & Antigravity)" trong MainPanel**
- [ ] **Step 2: Thêm các công tắc (Toggles) cho Codex, Antigravity, Chế độ Luôn hiển thị / Tự động, Chu kỳ cập nhật**
- [ ] **Step 3: Thêm nút Refresh ngay và hiển thị tình trạng kết nối thời gian thực**
- [ ] **Step 4: Kiểm tra giao diện và lưu trạng thái vào localStorage / backend**

---

### Task 4: Triển khai UI Dynamic Island (`src/views/WidgetIsland.vue`)

**Files:**
- Modify: `src/views/WidgetIsland.vue`

**Interfaces:**
- Consumes: Tauri event `ai-quota-event` và invoke `refresh_ai_quota`.

- [ ] **Step 1: Thêm component/template Quota Capsule cho Dynamic Island**
  - Compact: Hiển thị icon nhà cung cấp + % 5h còn lại + vạch màu trạng thái.
- [ ] **Step 2: Triển khai hiệu ứng Hover Popover (Glassmorphism HUD)**
  - Hiển thị đầy đủ 5-hour limit, Weekly limit, thời gian đếm ngược Reset, nút reload nhanh.
  - Hỗ trợ hiển thị song song nếu cả Codex & Antigravity đều bật.
- [ ] **Step 3: Xử lý logic hiển thị theo `display_mode` (Auto khi focus VSCode/Codex hoặc Always)**
- [ ] **Step 4: Kiểm tra animation và css layout**

---

### Task 5: Kiểm thử hoàn chỉnh & Xác minh (Verification)

**Files:**
- Test all components end-to-end.

- [ ] **Step 1: Chạy `npm run build` kiểm tra TypeScript và Vue template**
- [ ] **Step 2: Chạy `cargo check` / `cargo build` kiểm tra Backend Rust**
- [ ] **Step 3: Xác minh luồng dữ liệu và trải nghiệm hover trên Dynamic Island**
