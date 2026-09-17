import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRenderer } from 'vue';
import { PlaybackClock } from '../src/features/lyrics/clock';
import { computeFrameState } from '../src/features/lyrics/syncEngine';
import { useLyrics } from '../src/features/lyrics/useLyrics';
import { lyricScrollOffset, wordScrollOffset } from '../src/features/lyrics/scroll';

const sample = { trackKey: 'sample', source: 'fixture', fetchedAt: 0, syncType: 'line' as const,
    lines: [{ text: 'First', startMs: 1000, endMs: 2000 }, { text: 'Second', startMs: 3000, endMs: 4000 }] };

test('long silence after 2s hold and end of track do not retain an expired line', () => {
    const interlude = { trackKey: 'interlude', source: 'fixture', fetchedAt: 0, syncType: 'line' as const,
        lines: [{ text: 'First', startMs: 1000, endMs: 2000 }, { text: 'Second', startMs: 8000, endMs: 9000 }] };
    // Within 2s hold: First is still readable at 2500ms
    assert.equal(computeFrameState(interlude, 2500, 0).currentLine?.text, 'First');
    // After 2s hold expires during long interlude: null
    assert.equal(computeFrameState(interlude, 5000, 0).currentLine, null);
    assert.equal(computeFrameState(interlude, 5000).nextLine?.text, 'Second');
    // End of track at endMs: null
    assert.equal(computeFrameState(interlude, 9000, 1).currentLine, null);
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
    assert.ok(lyricScrollOffset(400, 200, 0.5) > 100);
    assert.equal(lyricScrollOffset(100, 200, 1), 0);
});

test('line-only scroll holds briefly, cruises, and slows before its deadline', () => {
    for (const duration of [600, 1500, 4000, 12000]) {
        const at = (p: number) => lyricScrollOffset(800, 200, p, duration);
        const early = at(0.3) - at(0.2);
        const late = at(0.7) - at(0.6);
        assert.ok(Math.abs(early - late) < 0.001, 'No slow start followed by a rush');
        const deadline = 1 - Math.min(350, duration * 0.15) / duration;
        assert.equal(at(deadline), 600);
        assert.equal(at(0), 0);
        assert.equal(at(0.02), 0, 'Hold the opening briefly');
        assert.ok(at(deadline) - at(deadline - 0.01) < at(0.5) - at(0.49), 'Ease out before stopping');
        assert.equal(at(1), 600);
        assert.ok(at(0.1) > 0, 'Begin moving before the first tenth of the cue');
        let previous = 0;
        for (let p = 0; p <= 1; p += 0.001) {
            const value = at(p);
            assert.ok(value >= previous && value <= 600);
            previous = value;
        }
    }
});

test('word scroll waits for a slow opening and remains smooth across word boundaries', () => {
    const words = [
        { startMs: 0, left: 0 }, { startMs: 2500, left: 60 },
        { startMs: 3000, left: 120 }, { startMs: 3500, left: 180 },
        { startMs: 4000, left: 240 }, { startMs: 4500, left: 320 },
    ];
    const at = (ms: number) => wordScrollOffset(400, 200, ms, 0, 5000, words);
    assert.equal(at(2000), 0, 'Do not outrun the sustained opening words');
    assert.ok(at(3250) > 0);
    assert.equal(at(5000), 200);
    let previous = 0;
    for (let ms = 0; ms <= 5000; ms += 5) {
        const current = at(ms);
        assert.ok(current >= previous - 1e-8 && current <= 200);
        assert.ok(current - previous < 2, 'No word-boundary jumps');
        previous = current;
    }
    for (const word of words.slice(1)) {
        const leftSpeed = at(word.startMs) - at(word.startMs - 1);
        const rightSpeed = at(word.startMs + 1) - at(word.startMs);
        assert.ok(Math.abs(leftSpeed - rightSpeed) < 0.002, 'Velocity stays continuous');
    }
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

test('line mode bridges small gaps seamlessly and leads in before singing', () => {
    const contiguous = {
        trackKey: 'contig', source: 'fixture', fetchedAt: 0, syncType: 'line' as const,
        lines: [
            { text: 'Line 1', startMs: 33000, endMs: 37500 },
            { text: 'Line 2', startMs: 37800, endMs: 42000 },
        ],
    };
    // 200ms before Line 1 starts: lead-in displays Line 1 early
    assert.equal(computeFrameState(contiguous, 32800).currentLine?.text, 'Line 1');

    // Gap between 37500 and 37800 (300ms gap): Line 1 smoothly holds until Line 2 takes over
    assert.equal(computeFrameState(contiguous, 37400).currentLine?.text, 'Line 1');

    // At 37600 (within 300ms lead-in of Line 2): Line 2 seamlessly takes over with 0ms blank
    assert.equal(computeFrameState(contiguous, 37600).currentLine?.text, 'Line 2');
});

