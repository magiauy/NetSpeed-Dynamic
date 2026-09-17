pub mod cache;
pub mod client;
pub mod models;

use models::{NormalizedLyrics, SyncType, TrackPreloadItem};

fn is_placeholder_artist(artist: &str) -> bool {
    let a = artist.trim().to_lowercase();
    a.is_empty()
        || a == "edge"
        || a == "chrome"
        || a == "potplayer"
        || a == "youtube music"
        || a == "spotify"
        || a == "apple music"
        || a == "browser"
        || a == "unknown artist"
        || a == "various artists"
}

fn is_music_meta_tag(s: &str) -> bool {
    let keywords = [
        "ost",
        "soundtrack",
        "sound track",
        "nhạc phim",
        "nhac phim",
        "theme song",
        "opening",
        "ending",
        "official",
        "mv",
        "video",
        "audio",
        "lyrics",
        "lyric",
        "visualizer",
        "live",
        "acoustic",
        "remix",
        "cover",
        "ver.",
        "version",
        "4k",
        "1080p",
        "hd",
        "feat.",
        "feat ",
        "ft.",
        "ft ",
        "prod.",
        "prod ",
    ];

    for kw in &keywords {
        if s.contains(kw) {
            return true;
        }
    }
    false
}

fn strip_brackets_metadata(s: &str) -> String {
    let mut result = s.to_string();
    let brackets = [('(', ')'), ('[', ']'), ('【', '】'), ('（', '）'), ('<', '>')];

    for (open_ch, close_ch) in brackets {
        let mut loop_count = 0;
        while loop_count < 10 {
            loop_count += 1;
            let open_pos = match result.find(open_ch) {
                Some(pos) => pos,
                None => break,
            };
            let close_pos = match result[open_pos..].find(close_ch) {
                Some(pos) => open_pos + pos,
                None => break,
            };

            let inside = &result[open_pos + open_ch.len_utf8()..close_pos];
            let inside_lower = inside.trim().to_lowercase();

            let is_meta = is_music_meta_tag(&inside_lower);

            if is_meta {
                let candidate = format!("{}{}", &result[..open_pos], &result[close_pos + close_ch.len_utf8()..]);
                let trimmed_candidate = candidate.trim();
                if !trimmed_candidate.is_empty() {
                    result = candidate;
                    continue;
                }
            }
            break;
        }
    }
    result
}

fn sanitize_artist_name(artist: &str, inferred: &str) -> String {
    let mut a = artist.trim().to_string();
    if is_placeholder_artist(&a) && !inferred.is_empty() {
        a = inferred.trim().to_string();
    }

    // Chuyển liên từ tiếng Việt ' và ' hoặc ' va ' thành ', ' (Ví dụ: 'An Vũ và Sino' -> 'An Vũ, Sino')
    for conj in [" và ", " Va ", " VA ", " va "] {
        if a.contains(conj) {
            a = a.replace(conj, ", ");
        }
    }

    if let Some(stripped) = a.strip_suffix(" - Topic") {
        a = stripped.trim().to_string();
    } else if let Some(stripped) = a.strip_suffix(" - topic") {
        a = stripped.trim().to_string();
    }

    for suffix in [" Official", " official", " VEVO", " vevo"] {
        if let Some(stripped) = a.strip_suffix(suffix) {
            a = stripped.trim().to_string();
            break;
        }
    }
    a
}

/// Làm sạch tên bài hát và ca sĩ (bỏ [Official MV], (Lyric Video), (OST...), tiền tố Now Playing, v.v.)
pub fn sanitize_song_and_artist(song: &str, artist: &str) -> (String, String) {
    let mut clean_song = song.trim().to_string();

    for prefix in ["正在播放: ", "正在播放：", "Now Playing: ", "Playing: ", "Bài hát: "] {
        if let Some(stripped) = clean_song.strip_prefix(prefix) {
            clean_song = stripped.trim().to_string();
            break;
        }
    }

    let mut inferred_artist = String::new();
    if clean_song.contains(" - ") {
        let parts: Vec<String> = clean_song
            .split(" - ")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 2 {
            let part1_lower = parts[1].to_lowercase();
            if is_music_meta_tag(&part1_lower) {
                clean_song = parts[0].clone();
            } else if is_placeholder_artist(artist) {
                inferred_artist = parts[0].clone();
                clean_song = parts[1].clone();
            }
        }
    }

    // Xóa các cụm metadata trong ngoặc đơn / ngoặc vuông (như OST, Soundtrack, Official MV...)
    clean_song = strip_brackets_metadata(&clean_song);

    // Xóa các cụm từ metadata còn sót lại không nằm trong ngoặc
    let raw_patterns = [
        "official music video", "official mv", "official audio", "official lyric video",
        "official video", "official visualizer", "lyric video", "lyrics video", "video lyrics",
        "original soundtrack", "acoustic version", "live session"
    ];

    let mut lower = clean_song.to_lowercase();
    for pat in &raw_patterns {
        if let Some(idx) = lower.find(pat) {
            clean_song.replace_range(idx..idx + pat.len(), "");
            lower = clean_song.to_lowercase();
        }
    }

    clean_song = clean_song
        .replace("[]", "")
        .replace("()", "")
        .replace("【】", "")
        .replace("（）", "")
        .trim_matches(|c: char| c == '-' || c == '|' || c == '_' || c == '/' || c == '\\' || c.is_whitespace())
        .to_string();

    // Chuẩn hóa khoảng trắng thừa
    clean_song = clean_song
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let clean_artist = sanitize_artist_name(artist, &inferred_artist);

    (clean_song, clean_artist)
}

/// Lệnh Tauri chính: Lấy lyric căn chỉnh (Ưu tiên Custom AI Server 100.73.90.79, Fallback LRCLIB)
#[tauri::command]
pub async fn fetch_normalized_lyrics(
    song_name: String,
    artist_name: String,
    _album_name: Option<String>,
    duration_ms: i64,
) -> Result<NormalizedLyrics, String> {
    let (clean_song, clean_artist) = sanitize_song_and_artist(&song_name, &artist_name);
    if clean_song.is_empty() {
        return Ok(NormalizedLyrics {
            track_key: String::new(),
            source: "none".to_string(),
            fetched_at: 0,
            sync_type: SyncType::None,
            lines: Vec::new(),
        });
    }

    let track_key = format!(
        "{}::{}::{}",
        clean_artist.to_lowercase(),
        clean_song.to_lowercase(),
        duration_ms / 1000
    );

    println!(
        "[Lyrics] Fetching for: '{} - {}' ({}s)",
        clean_artist,
        clean_song,
        duration_ms / 1000
    );

    // 1. Thử lấy từ LRCLIB trực tuyến (Synced LRC lines có timestamp [mm:ss.xx])
    if let Ok(Some(lrclib_lyrics)) = client::fetch_from_lrclib(&clean_song, &clean_artist, duration_ms / 1000).await {
        println!("[Lyrics] LRCLIB Success (Synced Line): {}", track_key);
        return Ok(lrclib_lyrics);
    }

    // 2. Fallback sang NetEase Music API (File LRC có đầy đủ timestamp [mm:ss.xx] cho từng câu)
    println!("[Lyrics] LRCLIB unavailable or missing. Falling back to NetEase LRC...");
    if let Ok(Some(netease_lyrics)) = client::fetch_from_netease_fallback(&clean_song, &clean_artist, duration_ms).await {
        println!("[Lyrics] NetEase LRC Fallback Success (Synced Line): {}", track_key);
        return Ok(netease_lyrics);
    }

    // 3. Trả về rỗng nếu không tìm thấy lời có timestamp
    let empty = NormalizedLyrics {
        track_key: track_key.clone(),
        source: "not_found".to_string(),
        fetched_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        sync_type: SyncType::None,
        lines: Vec::new(),
    };
    Ok(empty)
}

/// Lệnh Tauri: Gửi danh sách bài tiếp theo trong Playlist
#[tauri::command]
pub async fn preload_upcoming_playlist(_tracks: Vec<TrackPreloadItem>) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_soundtrack_and_ost() {
        let (song, artist) = sanitize_song_and_artist(
            "Phép Màu (Đàn Cá Gỗ Original Soundtrack)",
            "MAYDAYs",
        );
        assert_eq!(song, "Phép Màu");
        assert_eq!(artist, "MAYDAYs");

        let (song, artist) = sanitize_song_and_artist(
            "Phép Màu (Đàn Cá Gỗ OST)",
            "MAYDAYs",
        );
        assert_eq!(song, "Phép Màu");
        assert_eq!(artist, "MAYDAYs");

        let (song, artist) = sanitize_song_and_artist(
            "Phép Màu [Đàn Cá Gỗ OST]",
            "MAYDAYs",
        );
        assert_eq!(song, "Phép Màu");
        assert_eq!(artist, "MAYDAYs");

        let (song, artist) = sanitize_song_and_artist(
            "Phép Màu - Đàn Cá Gỗ OST",
            "MAYDAYs",
        );
        assert_eq!(song, "Phép Màu");
        assert_eq!(artist, "MAYDAYs");
    }

    #[test]
    fn test_sanitize_youtube_suffixes_and_placeholders() {
        let (song, artist) = sanitize_song_and_artist(
            "Phép Màu (Official Music Video)",
            "MAYDAYs - Topic",
        );
        assert_eq!(song, "Phép Màu");
        assert_eq!(artist, "MAYDAYs");

        let (song, artist) = sanitize_song_and_artist(
            "MAYDAYs - Phép Màu",
            "YouTube Music",
        );
        assert_eq!(song, "Phép Màu");
        assert_eq!(artist, "MAYDAYs");

        let (song, artist) = sanitize_song_and_artist(
            "Chàng Trai Bất Tử",
            "An Vũ và Sino",
        );
        assert_eq!(song, "Chàng Trai Bất Tử");
        assert_eq!(artist, "An Vũ, Sino");
    }

    #[tokio::test]
    async fn test_fetch_nang_co_mang_em_ve_synced_lrc() {
        let lyrics = fetch_normalized_lyrics(
            "Nắng có mang em về".to_string(),
            "Shartnuss".to_string(),
            None,
            254000,
        )
        .await
        .unwrap();

        assert!(!lyrics.lines.is_empty(), "Lyrics lines should not be empty");
        assert_eq!(lyrics.sync_type, SyncType::Line, "Must be synced line lyrics");
        // Verify every line has valid positive timestamp and duration
        for line in &lyrics.lines {
            assert!(line.start_ms >= 0, "Line start timestamp must be non-negative");
            assert!(line.end_ms > line.start_ms, "Line end timestamp must be greater than start");
        }

        // Verify signature line with its timestamp
        let found = lyrics.lines.iter().find(|l| l.text.contains("Liệu nắng có khiến em quay về"));
        assert!(found.is_some(), "Should contain signature lyric line");
        let sig = found.unwrap();
        assert!(sig.start_ms > 10000 && sig.start_ms < 20000, "First line starts around 13s, got {}ms", sig.start_ms);
    }
}

