import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRenderer } from 'vue';
import { PlaybackClock } from '../src/features/lyrics/clock';
import { computeFrameState } from '../src/features/lyrics/syncEngine';
import { useLyrics } from '../src/features/lyrics/useLyrics';
import { lyricScrollOffset } from '../src/features/lyrics/scroll';

const sample = { trackKey: 'sample', source: 'fixture', fetchedAt: 0, syncType: 'line' as const,
    lines: [{ text: 'First', startMs: 1000, endMs: 2000 }, { text: 'Second', startMs: 3000, endMs: 4000 }] };

test('silence and end of track do not retain an expired line', () => {
    assert.equal(computeFrameState(sample, 2500, 0).currentLine, null);
    assert.equal(computeFrameState(sample, 4500, 1).currentLine, null);
    assert.equal(computeFrameState(sample, 2500).nextLine?.text, 'Second');
});

test('plain lyrics are not treated as timed cues', () => {
    assert.equal(computeFrameState({ ...sample, syncType: 'plain' }, 1500).currentLine, null);
});

test('overlapping lines select the latest active start, independent of cursor history', () => {
    const lyrics = { ...sample, lines: [{ ...sample.lines[0], endMs: 5000 }, sample.lines[1]] };
    assert.equal(computeFrameState(lyrics, 3500, 0).activeLineIndex, 1);
});

test('paused seek smaller than drift tolerance still updates position', () => {
    const clock = new PlaybackClock();
    clock.reset(1000, false);
    clock.updateSnapshot(1250, false);
    assert.equal(clock.getPositionMs(), 1250);
});

test('word highlighting uses timestamps and rewinds after seek, not character count', () => {
    const lyric = { ...sample, syncType: 'word' as const, lines: [{ text: 'A long', startMs: 1000, endMs: 5000,
        words: [{ text: 'A ', startMs: 1000, endMs: 1100 }, { text: 'long', startMs: 2000, endMs: 5000 }] }] };
    assert.equal(computeFrameState(lyric, 1050).wordProgress, 0.5);
    assert.equal(computeFrameState(lyric, 1500).wordProgress, 1);
    assert.equal(computeFrameState(lyric, 3500, 0, 0).wordProgress, 0.5);
    assert.equal(computeFrameState(lyric, 1050, 0, 1).activeWordIndex, 0);
});

function mountLyrics(invoke: () => Promise<unknown>) {
    (globalThis as any).window = { __TAURI_INTERNALS__: { invoke } };
    (globalThis as any).requestAnimationFrame = () => 1;
    (globalThis as any).cancelAnimationFrame = () => {};
    const renderer = createRenderer<any, any>({
        createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
        insert() {}, remove() {}, setText() {}, setElementText() {}, patchProp() {},
        parentNode: () => null, nextSibling: () => null,
    });
    let lyrics!: ReturnType<typeof useLyrics>;
    const app = renderer.createApp({ setup() { lyrics = useLyrics(); return () => null; } });
    app.mount({});
    return { lyrics, unmount: () => app.unmount() };
}
const track = { title: 'Sample', artist: 'Test', durationMs: 5000, positionMs: 1500, playing: false };

test('Phep Mau OST 267s: every real cue boundary selects the right line, including backward seeks', () => {
    // Timing-only fixture: YouTube OkXnZSafFns / LRCLIB 25781857, checked 2026-09-17.
    // No copyrighted lyric text and no invented word timestamps.
    const starts = [3320,10120,16720,23190,30610,44170,50850,57540,61220,64150,67500,70880,77470,84180,87950,91180,98360,104920,111250,114660,117910,124550,130850,137430,144180,147940,150890,154130,157440,164180,170850,174640,177930,187250,192080,197090,217450,224250,230870,234620,237910,244610,250200];
    const fixture = { ...sample, lines: starts.map((startMs, index) => ({ text: `Cue ${index}`, startMs, endMs: starts[index + 1] ?? 267000 })) };
    let hint = -1;
    for (const index of [...starts.keys(), ...starts.keys()].reverse()) {
        const frame = computeFrameState(fixture, starts[index], hint);
        assert.equal(frame.activeLineIndex, index);
        assert.equal(frame.activeWordIndex, -1);
        hint = frame.activeLineIndex;
    }
    assert.equal(computeFrameState(fixture, 267000, hint).currentLine, null);
});

test('playback rate and pause use the same clock without accumulated drift', (t) => {
    let now = 0;
    t.mock.method(performance, 'now', () => now);
    const clock = new PlaybackClock();
    clock.updateSnapshot(1000, true, true, 2);
    now = 500;
    assert.equal(clock.getPositionMs(), 2000);
    clock.updateSnapshot(2000, false);
    now = 2000;
    assert.equal(clock.getPositionMs(), 2000);
});

test('normal playback corrects an audible 250ms drift', (t) => {
    t.mock.method(performance, 'now', () => 0);
    const clock = new PlaybackClock();
    clock.reset(1000, true);
    clock.updateSnapshot(1250, true);
    assert.equal(clock.getPositionMs(), 1250);
});

test('lyric delay applies to cue selection, without changing playback position', async () => {
    const { lyrics, unmount } = mountLyrics(async () => sample);
    try {
        await lyrics.fetchLyricsForTrack(track);
        lyrics.setDelayMs(1000);
        assert.equal(lyrics.currentLine.value, null);
        assert.equal(lyrics.clock.getPositionMs(), 1500);
        lyrics.setDelayMs(-2000);
        assert.equal(lyrics.currentLine.value?.text, 'Second');
    } finally { unmount(); }
});

test('scroll follows line time and stops at the readable viewport edge', () => {
    assert.equal(lyricScrollOffset(400, 200, 0), 0);
    assert.equal(lyricScrollOffset(400, 200, 1), 200);
    assert.equal(lyricScrollOffset(400, 200, 0.475), 100);
    assert.equal(lyricScrollOffset(100, 200, 1), 0);
});

test('external lyric takeover invalidates a pending HTTP result', async () => {
    let finish!: (value: unknown) => void;
    const { lyrics, unmount } = mountLyrics(() => new Promise(resolve => { finish = resolve; }));
    try {
        const pending = lyrics.fetchLyricsForTrack(track);
        lyrics.replaceLyrics({ ...sample, source: 'ws', trackKey: 'ws-track' }, 3500, false);
        finish(sample);
        await pending;
        assert.equal(lyrics.lyrics.value?.source, 'ws');
        assert.equal(lyrics.currentLine.value?.text, 'Second');
    } finally { unmount(); }
});

test('network failure retries after cooldown and recovers on the same track', async (t) => {
    let now = 0;
    t.mock.method(Date, 'now', () => now);
    let requests = 0;
    const { lyrics, unmount } = mountLyrics(async () => {
        requests++;
        if (requests === 1) throw new Error('503');
        return sample;
    });
    try {
        await lyrics.fetchLyricsForTrack(track);
        assert.equal(lyrics.lyrics.value, null);
        await lyrics.fetchLyricsForTrack(track);
        assert.equal(lyrics.lyrics.value, null);
        now = 16000;
        await lyrics.fetchLyricsForTrack(track);
        assert.equal(lyrics.currentLine.value?.text, 'First');
        assert.equal(lyrics.error.value, null);
    } finally { unmount(); }
});

test('loading a track initializes playback at its supplied position', async () => {
    const { lyrics, unmount } = mountLyrics(async () => sample);
    try {
        await lyrics.fetchLyricsForTrack(track);
        assert.equal(lyrics.currentLine.value?.text, 'First');
        assert.equal(lyrics.clock.getPositionMs(), 1500);
    } finally { unmount(); }
});

test('changing track clears old cues immediately and ignores late responses after clear', async () => {
    let finish!: (value: unknown) => void;
    let calls = 0;
    const { lyrics, unmount } = mountLyrics(() => ++calls === 1 ? Promise.resolve(sample) : new Promise(resolve => { finish = resolve; }));
    try {
        lyrics.updatePlayback(1500, false, true);
        await lyrics.fetchLyricsForTrack(track);
        const pending = lyrics.fetchLyricsForTrack({ ...track, title: 'Other' });
        assert.equal(lyrics.currentLine.value, null);
        await lyrics.fetchLyricsForTrack({ ...track, title: '' });
        finish(sample);
        await pending;
        assert.equal(lyrics.lyrics.value, null);
        assert.equal(lyrics.isLoading.value, false);
    } finally { unmount(); }
});
