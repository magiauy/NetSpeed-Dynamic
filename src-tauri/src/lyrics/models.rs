use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncType {
    Word,
    Line,
    Plain,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedLyricWord {
    pub text: String,
    #[serde(rename = "startMs")]
    pub start_ms: i64,
    #[serde(rename = "endMs")]
    pub end_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedLyricLine {
    pub text: String,
    #[serde(rename = "startMs")]
    pub start_ms: i64,
    #[serde(rename = "endMs")]
    pub end_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<NormalizedLyricWord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedLyrics {
    #[serde(rename = "trackKey")]
    pub track_key: String,
    pub source: String,
    #[serde(rename = "fetchedAt")]
    pub fetched_at: u64,
    #[serde(rename = "syncType")]
    pub sync_type: SyncType,
    pub lines: Vec<NormalizedLyricLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPreloadItem {
    pub title: String,
    pub artist: String,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<i64>,
    #[serde(rename = "youtubeUrl")]
    pub youtube_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreloadRequest {
    pub tracks: Vec<TrackPreloadItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LrclibResponse {
    #[serde(rename = "plainLyrics")]
    pub plain_lyrics: Option<String>,
    #[serde(rename = "syncedLyrics")]
    pub synced_lyrics: Option<String>,
}
