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

/// 1. Tìm và lấy lời bài hát có timestamp từ LRCLIB
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

/// 2. Fallback trực tuyến: Lấy lời từ YouTube Music (LyricFind qua InnerTube API)
pub async fn fetch_from_ytmusic_fallback(
    title: &str,
    artist: &str,
    duration_ms: i64,
) -> Result<Option<NormalizedLyrics>, String> {
    let client = get_client();
    let query = format!("{} {}", title, artist).trim().to_string();

    // 1. Tìm videoId trên YouTube Music
    let search_url = "https://music.youtube.com/youtubei/v1/search";
    let search_payload = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20240101.01.00"
            }
        },
        "query": query
    });

    let search_res = client
        .post(search_url)
        .json(&search_payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !search_res.status().is_success() {
        return Ok(None);
    }

    let search_text = search_res.text().await.map_err(|e| e.to_string())?;

    let mut candidate_ids: Vec<String> = Vec::new();
    let pattern = "\"videoId\": \"";
    let pattern2 = "\"videoId\":\"";
    for pat in [pattern, pattern2] {
        for part in search_text.split(pat).skip(1) {
            if let Some(end) = part.find('"') {
                let vid = &part[..end];
                if vid.len() == 11 && !candidate_ids.contains(&vid.to_string()) {
                    candidate_ids.push(vid.to_string());
                    if candidate_ids.len() >= 6 {
                        break;
                    }
                }
            }
        }
    }

    // 2. Với mỗi candidate, kiểm tra endpoint Next để lấy browseId của tab Lyrics (MPLYt...)
    for video_id in candidate_ids {
        let next_url = "https://music.youtube.com/youtubei/v1/next";
        let next_payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240101.01.00"
                }
            },
            "videoId": video_id
        });

        let next_res = match client.post(next_url).json(&next_payload).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let next_text = match next_res.text().await {
            Ok(t) => t,
            _ => continue,
        };

        let browse_prefix = "MPLYt";
        let browse_id = match next_text.find(browse_prefix) {
            Some(idx) => {
                let slice = &next_text[idx..];
                let end = slice
                    .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                    .unwrap_or(slice.len());
                &slice[..end]
            }
            None => continue,
        };

        // 3. Tải lời bài hát từ tab Browse
        let browse_url = "https://music.youtube.com/youtubei/v1/browse";
        let browse_payload = serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20240101.01.00"
                }
            },
            "browseId": browse_id
        });

        let browse_res = match client.post(browse_url).json(&browse_payload).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let browse_json: serde_json::Value = match browse_res.json().await {
            Ok(v) => v,
            _ => continue,
        };

        let section = &browse_json["contents"]["sectionListRenderer"]["contents"][0]["musicDescriptionShelfRenderer"];
        let mut lyrics_str = String::new();
        if let Some(runs) = section["description"]["runs"].as_array() {
            for r in runs {
                if let Some(txt) = r["text"].as_str() {
                    lyrics_str.push_str(txt);
                }
            }
        }

        let lyrics_trimmed = lyrics_str.trim();
        if !lyrics_trimmed.is_empty() {
            let lines: Vec<NormalizedLyricLine> = lyrics_trimmed
                .split('\n')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .map(|text| NormalizedLyricLine {
                    text,
                    start_ms: 0,
                    end_ms: 0,
                    words: None,
                })
                .collect();

            if !lines.is_empty() {
                let track_key = format!(
                    "{}::{}::{}",
                    artist.to_lowercase(),
                    title.to_lowercase(),
                    duration_ms / 1000
                );
                return Ok(Some(NormalizedLyrics {
                    track_key,
                    source: "youtube_music".to_string(),
                    fetched_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    sync_type: SyncType::Plain,
                    lines,
                }));
            }
        }
    }

    Ok(None)
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
        source: "lrclib".to_string(),
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
