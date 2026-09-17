use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::sync::OnceLock;
use std::time::Duration;

use super::models::{LrclibResponse, NormalizedLyricLine, NormalizedLyrics, SyncType};

const USER_AGENT_VAL: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn get_client() -> reqwest::Client {
    HTTP_CLIENT
        .get_or_init(|| {
            let mut headers = HeaderMap::new();
            if let Ok(ua) = HeaderValue::from_str(USER_AGENT_VAL) {
                headers.insert(USER_AGENT, ua);
            }
            reqwest::Client::builder()
                .default_headers(headers)
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default()
        })
        .clone()
}

/// 1. Tìm và lấy lời bài hát có timestamp từ LRCLIB (chỉ chấp nhận synced lyrics có timestamp)
pub async fn fetch_from_lrclib(
    title: &str,
    artist: &str,
    duration_sec: i64,
) -> Result<Option<NormalizedLyrics>, String> {
    let client = get_client();
    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        urlencoding::encode(title),
        urlencoding::encode(artist)
    );
    if duration_sec > 0 {
        url.push_str(&format!("&duration={}", duration_sec));
    }

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => {
            // Thử search nếu get chính xác thất bại
            let search_url = format!(
                "https://lrclib.net/api/search?q={}",
                urlencoding::encode(&format!("{} {}", title, artist))
            );
            let search_resp = client.get(&search_url).send().await.map_err(|e| e.to_string())?;
            if !search_resp.status().is_success() {
                return Ok(None);
            }
            let items: Vec<LrclibResponse> = search_resp.json().await.map_err(|e| e.to_string())?;
            for item in items {
                if let Some(lyrics) = parse_lrclib_synced_response(item, title, artist, duration_sec * 1000) {
                    return Ok(Some(lyrics));
                }
            }
            return Ok(None);
        }
    };

    if !resp.status().is_success() {
        return Ok(None);
    }

    let data: LrclibResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_lrclib_synced_response(data, title, artist, duration_sec * 1000))
}

/// 2. Fallback sang NetEase Music API (cung cấp file LRC đầy đủ mốc thời gian [mm:ss.xx])
pub async fn fetch_from_netease_fallback(
    title: &str,
    artist: &str,
    duration_ms: i64,
) -> Result<Option<NormalizedLyrics>, String> {
    let client = get_client();
    let query = format!("{} {}", title, artist).trim().to_string();

    // 1. Tìm kiếm danh sách bài hát
    let search_url = format!(
        "http://music.163.com/api/search/get?s={}&type=1&limit=6",
        urlencoding::encode(&query)
    );

    let search_res = match client.get(&search_url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Ok(None),
    };

    let search_json: serde_json::Value = match search_res.json().await {
        Ok(v) => v,
        _ => return Ok(None),
    };

    let songs = match search_json["result"]["songs"].as_array() {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(None),
    };

    // 2. Duyệt qua các kết quả để lấy file LRC có timestamp
    for song in songs {
        let song_id = match song["id"].as_i64() {
            Some(id) => id,
            None => continue,
        };

        let lyric_url = format!(
            "http://music.163.com/api/song/lyric?id={}&lv=1&kv=1&tv=-1",
            song_id
        );

        let lyric_res = match client.get(&lyric_url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let lyric_json: serde_json::Value = match lyric_res.json().await {
            Ok(v) => v,
            _ => continue,
        };

        if let Some(lrc_text) = lyric_json["lrc"]["lyric"].as_str() {
            if let Some(lyrics) = parse_lrc_string(lrc_text, title, artist, duration_ms, "netease") {
                return Ok(Some(lyrics));
            }
        }
    }

    Ok(None)
}

/// Parse chuỗi chuẩn LRC chứa các mốc thời gian [mm:ss.xx] thành NormalizedLyrics
pub fn parse_lrc_string(
    raw_text: &str,
    title: &str,
    artist: &str,
    duration_ms: i64,
    source: &str,
) -> Option<NormalizedLyrics> {
    let raw_lines: Vec<&str> = raw_text
        .split('\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut timed_lines: Vec<(i64, String)> = Vec::new();

    for line_str in raw_lines {
        // Kiểm tra xem dòng có định dạng [mm:ss.xx] không
        if line_str.starts_with('[') && line_str.contains(']') {
            let parts: Vec<&str> = line_str.splitn(2, ']').collect();
            if parts.len() == 2 {
                let time_part = &parts[0][1..];
                let text = parts[1].trim().to_string();

                // Bỏ qua các thẻ metadata LRC như [ti:...], [ar:...], [al:...], [by:...]
                if let Some(start_ms) = parse_lrc_time(time_part) {
                    // Bỏ qua dòng rỗng hoặc chỉ chứa nhãn phân đoạn không phải lời hát
                    if !text.is_empty() && !is_section_header_label(&text) {
                        timed_lines.push((start_ms, text));
                    }
                }
            }
        }
    }

    // Yêu cầu bắt buộc: Phải có ít nhất các dòng có mốc thời gian thực sự
    if timed_lines.is_empty() {
        return None;
    }

    // Sắp xếp các dòng theo thứ tự thời gian start_ms
    timed_lines.sort_by_key(|(start, _)| *start);

    let total = timed_lines.len();
    let mut lines: Vec<NormalizedLyricLine> = Vec::with_capacity(total);

    for (i, (start_ms, text)) in timed_lines.iter().enumerate() {
        let next_start = if i + 1 < total {
            Some(timed_lines[i + 1].0)
        } else {
            None
        };

        let end_ms = next_start.unwrap_or(start_ms + 4000);
        lines.push(NormalizedLyricLine {
            text: text.clone(),
            start_ms: *start_ms,
            end_ms: end_ms.max(*start_ms + 500),
            words: None,
        });
    }

    let track_key = format!(
        "{}::{}::{}",
        artist.to_lowercase(),
        title.to_lowercase(),
        duration_ms / 1000
    );

    Some(NormalizedLyrics {
        track_key,
        source: source.to_string(),
        fetched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        sync_type: SyncType::Line,
        lines,
    })
}

fn parse_lrclib_synced_response(
    data: LrclibResponse,
    title: &str,
    artist: &str,
    duration_ms: i64,
) -> Option<NormalizedLyrics> {
    // Chỉ chấp nhận synced_lyrics có mốc thời gian, từ chối plain_lyrics
    let synced_text = data.synced_lyrics?;
    parse_lrc_string(&synced_text, title, artist, duration_ms, "lrclib")
}

fn parse_lrc_time(time_str: &str) -> Option<i64> {
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let mins: f64 = parts[0].parse().ok()?;
    let secs: f64 = parts[1].parse().ok()?;
    Some(((mins * 60.0 + secs) * 1000.0) as i64)
}

fn is_section_header_label(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    lower == "điệp khúc："
        || lower == "điệp khúc:"
        || lower == "giai đoạn hai："
        || lower == "giai đoạn hai:"
        || lower == "đảo ngược góc nhìn："
        || lower == "đảo ngược góc nhìn:"
        || lower == "kết thúc："
        || lower == "kết thúc:"
        || lower == "intro"
        || lower == "outro"
        || lower == "chorus"
        || lower == "verse 1"
        || lower == "verse 2"
}
