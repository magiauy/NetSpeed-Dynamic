export type SyncType = 'word' | 'line' | 'plain' | 'none';

export interface NormalizedLyricWord {
    text: string;
    startMs: number;
    endMs: number;
}

export interface NormalizedLyricLine {
    text: string;
    startMs: number;
    endMs: number;
    words?: NormalizedLyricWord[];
}

export interface NormalizedLyrics {
    trackKey: string;
    source: string;
    fetchedAt: number;
    syncType: SyncType;
    lines: NormalizedLyricLine[];
}

export interface PlaybackClockSnapshot {
    positionMs: number;
    receivedAt: number;
    playing: boolean;
}

export interface PlaybackTrack {
    title: string;
    artist: string;
    album?: string;
    durationMs: number;
    positionMs: number;
    playing: boolean;
    appId?: string;
}

export interface LyricFrameState {
    activeLineIndex: number;
    activeWordIndex: number;
    wordProgress: number;
    lineProgress: number;
    currentLine: NormalizedLyricLine | null;
    nextLine: NormalizedLyricLine | null;
}
