use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use std::sync::OnceLock;
use std::time::Duration;

use super::models::{LrclibResponse, NormalizedLyricLine, NormalizedLyrics, PreloadRequest, SyncType, TrackPreloadItem};

const CUSTOM_API_BASE: &str = "http://100.73.90.79:8765";
const USER_AGENT_VAL: &str = "NetSpeedDynamic/1.0";

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
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default()
        })
        .clone()
}

/// 1. Gọi đến máy chủ AI Lyric Alignment của bạn (100.73.90.79)
pub async fn fetch_from_custom_server(
    title: &str,
    artist: &str,
    duration_ms: i64,
) -> Result<Option<NormalizedLyrics>, String> {
    let client = get_client();
    let url = format!(
        "{}/api/lyrics?title={}&artist={}&duration_ms={}",
        CUSTOM_API_BASE,
        urlencoding::encode(title),
        urlencoding::encode(artist),
        duration_ms
    );

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => return Err(format!("Custom server connect error: {}", e)),
    };

    if !resp.status().is_success() {
        return Ok(None);
    }

    match resp.json::<NormalizedLyrics>().await {
        Ok(data) => {
            if data.sync_type != SyncType::None && !data.lines.is_empty() {
                Ok(Some(data))
            } else {
                Ok(None)
            }
        }
        Err(e) => Err(format!("Failed to parse custom server response: {}", e)),
    }
}

/// 2. Gửi danh sách 5 bài tiếp theo trong playlist để pre-gen
pub async fn send_preload_queue(tracks: Vec<TrackPreloadItem>) -> Result<(), String> {
    let client = get_client();
    let url = format!("{}/api/queue/preload", CUSTOM_API_BASE);

    let payload = PreloadRequest { tracks };
    let _ = client.post(&url).json(&payload).send().await;
    Ok(())
}

/// 3. Fallback trực tiếp về LRCLIB nếu máy chủ AI offline hoặc chưa có lyric
pub async fn fetch_from_lrclib_fallback(
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
            if let Some(first) = items.into_iter().next() {
                return Ok(parse_lrclib_response(first, title, artist, duration_sec * 1000));
            }
            return Ok(None);
        }
    };

    if !resp.status().is_success() {
        return Ok(None);
    }

    let data: LrclibResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse_lrclib_response(data, title, artist, duration_sec * 1000))
}

fn parse_lrclib_response(
    data: LrclibResponse,
    title: &str,
    artist: &str,
    duration_ms: i64,
) -> Option<NormalizedLyrics> {
    let raw_text = data.synced_lyrics.or(data.plain_lyrics)?;
    let mut lines: Vec<NormalizedLyricLine> = Vec::new();

    let mut is_synced = false;
    let raw_lines: Vec<&str> = raw_text.split('\n').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

    for (i, line_str) in raw_lines.iter().enumerate() {
        if line_str.starts_with('[') && line_str.contains(']') {
            let parts: Vec<&str> = line_str.splitn(2, ']').collect();
            if parts.len() == 2 {
                let time_part = &parts[0][1..];
                let text = parts[1].trim().to_string();
                if let Some(start_ms) = parse_lrc_time(time_part) {
                    is_synced = true;
                    let next_start = raw_lines.get(i + 1).and_then(|nl| {
                        if nl.starts_with('[') && nl.contains(']') {
                            let np: Vec<&str> = nl.splitn(2, ']').collect();
                            parse_lrc_time(&np[0][1..])
                        } else {
                            None
                        }
                    });

                    let end_ms = next_start.unwrap_or(start_ms + 4000);
                    lines.push(NormalizedLyricLine {
                        text,
                        start_ms,
                        end_ms,
                        words: None,
                    });
                    continue;
                }
            }
        }

        // Plain line fallback
        lines.push(NormalizedLyricLine {
            text: line_str.to_string(),
            start_ms: 0,
            end_ms: 0,
            words: None,
        });
    }

    let track_key = format!("{}::{}::{}", artist.to_lowercase(), title.to_lowercase(), duration_ms / 1000);
    Some(NormalizedLyrics {
        track_key,
        source: "lrclib_fallback".to_string(),
        fetched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        sync_type: if is_synced { SyncType::Line } else { SyncType::Plain },
        lines,
    })
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
