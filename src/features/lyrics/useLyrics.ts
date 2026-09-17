import { ref, shallowRef, computed, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type {
    LyricFrameState,
    NormalizedLyrics,
    PlaybackTrack,
    SyncType,
} from './types';
import { PlaybackClock } from './clock';
import { computeFrameState } from './syncEngine';

export function useLyrics() {
    const lyrics = shallowRef<NormalizedLyrics | null>(null);
    const isLoading = ref(false);
    const error = ref<string | null>(null);

    const clock = new PlaybackClock();

    const frameState = shallowRef<LyricFrameState>({
        activeLineIndex: -1,
        activeWordIndex: -1,
        wordProgress: 0,
        lineProgress: 0,
        currentLine: null,
        nextLine: null,
    });

    let currentReqId = 0;
    let currentTrackKey = '';
    let rafId: number | null = null;
    let isLoopRunning = false;
    let delayMs = 0;
    let retryAfter = 0;
    const playbackPositionMs = ref(0);
    const renderFrame = () => {
        playbackPositionMs.value = clock.getPositionMs();
        frameState.value = computeFrameState(lyrics.value, playbackPositionMs.value - delayMs,
            frameState.value.activeLineIndex, frameState.value.activeWordIndex);
    };
    const setDelayMs = (value: number) => {
        delayMs = Number.isFinite(value) ? value : 0;
        renderFrame();
    };

    const syncType = computed<SyncType>(() => lyrics.value?.syncType || 'none');
    const hasLyrics = computed(() => !!lyrics.value && lyrics.value.lines.length > 0);
    const currentLine = computed(() => frameState.value.currentLine);
    const activeLineIndex = computed(() => frameState.value.activeLineIndex);
    const activeWordIndex = computed(() => frameState.value.activeWordIndex);
    const wordProgress = computed(() => frameState.value.wordProgress);

    const renderTick = () => {
        if (!isLoopRunning) return;

        renderFrame();

        rafId = requestAnimationFrame(renderTick);
    };

    const startRenderLoop = () => {
        if (isLoopRunning) return;
        isLoopRunning = true;
        rafId = requestAnimationFrame(renderTick);
    };

    const stopRenderLoop = () => {
        isLoopRunning = false;
        if (rafId !== null) {
            cancelAnimationFrame(rafId);
            rafId = null;
        }
    };

    /**
     * Updates playback state and synchronizes clock
     */
    const updatePlayback = (positionMs: number, playing: boolean, forceReset = false, playbackRate?: number) => {
        clock.updateSnapshot(positionMs, playing, forceReset, playbackRate);
        renderFrame();

        if (playing) {
            startRenderLoop();
        } else {
            stopRenderLoop();
        }
    };

    /**
     * Fetches lyrics with generation ID to avoid race conditions on track skipping
     */
    const fetchLyricsForTrack = async (track: PlaybackTrack, force = false) => {
        const title = track.title?.trim() || '';
        const artist = track.artist?.trim() || '';
        const album = track.album?.trim() || '';
        const durationMs = track.durationMs || 0;

        if (!title) {
            clearLyrics();
            return;
        }

        const newTrackKey = `${artist.toLowerCase()}::${title.toLowerCase()}::${Math.floor(durationMs / 1000)}`;

        if (!force && newTrackKey === currentTrackKey && (lyrics.value || isLoading.value || Date.now() < retryAfter)) {
            return;
        }

        if (newTrackKey !== currentTrackKey) {
            lyrics.value = null;
            frameState.value = computeFrameState(null, 0);
            clock.reset();
            updatePlayback(track.positionMs, track.playing, true);
        }
        currentTrackKey = newTrackKey;
        const reqId = ++currentReqId;
        isLoading.value = true;
        error.value = null;

        try {
            const result = await invoke<NormalizedLyrics>('fetch_normalized_lyrics', {
                songName: title,
                artistName: artist,
                albumName: album || null,
                durationMs,
            });

            // Race condition check: only apply if this request is still the active one
            if (reqId === currentReqId) {
                lyrics.value = result;
                // Recompute frame immediately with current clock position
                renderFrame();
            }
        } catch (err: any) {
            if (reqId === currentReqId) {
                error.value = err?.toString() || 'Failed to fetch lyrics';
                lyrics.value = null;
                frameState.value = computeFrameState(null, 0);
                retryAfter = Date.now() + 15000;
            }
        } finally {
            if (reqId === currentReqId) {
                isLoading.value = false;
            }
        }
    };

    /**
     * Clear lyrics when player stopped or switched
     */
    const clearLyrics = () => {
        lyrics.value = null;
        currentTrackKey = '';
        currentReqId++;
        isLoading.value = false;
        error.value = null;
        clock.reset();
        playbackPositionMs.value = 0;
        retryAfter = 0;
        frameState.value = computeFrameState(null, 0);
        stopRenderLoop();
    };

    const replaceLyrics = (result: NormalizedLyrics, positionMs: number, playing: boolean) => {
        clearLyrics();
        lyrics.value = result;
        currentTrackKey = result.trackKey;
        updatePlayback(positionMs, playing, true);
    };

    onUnmounted(() => {
        clearLyrics();
    });

    const preloadUpcomingTracks = async (tracks: PlaybackTrack[]) => {
        if (!tracks || !tracks.length) return;
        try {
            const payload = tracks.slice(0, 5).map(t => ({
                title: t.title?.trim() || '',
                artist: t.artist?.trim() || '',
                durationMs: t.durationMs || 0,
                youtubeUrl: t.appId || null,
            })).filter(t => t.title);

            if (payload.length > 0) {
                await invoke('preload_upcoming_playlist', { tracks: payload });
            }
        } catch (_) {}
    };

    return {
        lyrics,
        syncType,
        hasLyrics,
        currentLine,
        activeLineIndex,
        activeWordIndex,
        wordProgress,
        frameState,
        isLoading,
        error,
        clock,
        playbackPositionMs,
        setDelayMs,
        replaceLyrics,
        fetchLyricsForTrack,
        preloadUpcomingTracks,
        updatePlayback,
        clearLyrics,
        startRenderLoop,
        stopRenderLoop,
    };
}
