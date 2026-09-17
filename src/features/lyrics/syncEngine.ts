import type {
    LyricFrameState,
    NormalizedLyricLine,
    NormalizedLyricWord,
    NormalizedLyrics,
} from './types';

function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
}

const LINE_LEAD_IN_MS = 300; // Hiện câu mới sớm 300ms trước khi bắt đầu hát để kịp đọc
const LINE_HOLD_MS = 2000; // Giữ câu cũ thêm 2000ms (2s) sau khi kết thúc

/**
 * Calculates the effective display window for a line, providing:
 * 1. Lead-in: displays slightly before singing starts (e.g. 300ms).
 * 2. Hold-over: holds the previous line for ~2s or until the next line starts.
 */
export function getEffectiveLineWindow(
    lines: NormalizedLyricLine[],
    index: number,
): { start: number; end: number } {
    const line = lines[index];
    const total = lines.length;
    const next = index + 1 < total ? lines[index + 1] : null;

    // Start time: line begins displaying early by LINE_LEAD_IN_MS
    const start = Math.max(0, line.startMs - LINE_LEAD_IN_MS);

    // End time: hold for 2s or until the next line starts displaying
    let end: number;
    if (next) {
        const nextStart = Math.max(line.startMs, next.startMs - LINE_LEAD_IN_MS);
        end = Math.min(line.endMs + LINE_HOLD_MS, nextStart);
    } else {
        end = line.endMs;
    }

    return { start, end };
}

/**
 * Finds the index of the active lyric line for the given timestamp.
 * Uses sequential cursor hint for O(1) in playback, fallback to binary search on seek.
 */
export function findActiveLineIndex(
    lines: NormalizedLyricLine[],
    positionMs: number,
    hintIndex = -1,
): number {
    if (!lines || lines.length === 0) return -1;
    const total = lines.length;

    // Before the very first line starts (including lead-in)
    const firstWin = getEffectiveLineWindow(lines, 0);
    if (positionMs < firstWin.start) return -1;

    // Fast-path 1: Same line as previous frame
    if (hintIndex >= 0 && hintIndex < total) {
        const win = getEffectiveLineWindow(lines, hintIndex);
        if (positionMs >= win.start && positionMs < win.end
            && (hintIndex + 1 === total || positionMs < getEffectiveLineWindow(lines, hintIndex + 1).start)) {
            return hintIndex;
        }

        // Fast-path 2: Advance to next sequential line
        if (hintIndex + 1 < total) {
            const nextWin = getEffectiveLineWindow(lines, hintIndex + 1);
            if (positionMs >= nextWin.start && positionMs < nextWin.end
                && (hintIndex + 2 === total || positionMs < getEffectiveLineWindow(lines, hintIndex + 2).start)) {
                return hintIndex + 1;
            }
        }
    }

    // Binary search: find largest index where effective start <= positionMs
    let low = 0;
    let high = total - 1;
    let result = -1;

    while (low <= high) {
        const mid = (low + high) >> 1;
        const win = getEffectiveLineWindow(lines, mid);
        if (win.start <= positionMs) {
            result = mid;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }

    // Verify the line has not expired past its effective end
    while (result >= 0) {
        const win = getEffectiveLineWindow(lines, result);
        if (positionMs < win.end) {
            break;
        }
        result--;
    }

    return result;
}

/**
 * Finds the active word index within a line's words.
 */
export function findActiveWordIndex(
    words: NormalizedLyricWord[],
    positionMs: number,
    hintIndex = -1,
): number {
    if (!words || words.length === 0) return -1;
    if (positionMs < words[0].startMs) return -1;

    const total = words.length;

    // Fast-path: check hintIndex and hintIndex + 1
    if (hintIndex >= 0 && hintIndex < total) {
        const current = words[hintIndex];
        if (positionMs >= current.startMs && positionMs < current.endMs) {
            return hintIndex;
        }
        if (hintIndex + 1 < total) {
            const next = words[hintIndex + 1];
            if (positionMs >= next.startMs && positionMs < next.endMs) {
                return hintIndex + 1;
            }
        }
    }

    // Words lists per line are short (typically 2-15 words), linear or binary search
    let result = -1;
    for (let i = 0; i < total; i++) {
        if (words[i].startMs <= positionMs) {
            result = i;
        } else {
            break;
        }
    }

    return result;
}

/**
 * Computes complete frame state for current playback timestamp.
 */
export function computeFrameState(
    lyrics: NormalizedLyrics | null,
    positionMs: number,
    lastLineIndex = -1,
    lastWordIndex = -1,
): LyricFrameState {
    if (!lyrics || lyrics.syncType === 'plain' || lyrics.syncType === 'none'
        || !lyrics.lines || lyrics.lines.length === 0) {
        return {
            activeLineIndex: -1,
            activeWordIndex: -1,
            wordProgress: 0,
            lineProgress: 0,
            currentLine: null,
            nextLine: null,
        };
    }

    const lines = lyrics.lines;
    const lineIndex = findActiveLineIndex(lines, positionMs, lastLineIndex);

    if (lineIndex < 0 || lineIndex >= lines.length) {
        return {
            activeLineIndex: -1,
            activeWordIndex: -1,
            wordProgress: 0,
            lineProgress: 0,
            currentLine: null,
            nextLine: lines.find((_, idx) => getEffectiveLineWindow(lines, idx).start > positionMs) || null,
        };
    }

    const currentLine = lines[lineIndex];
    const nextLine = lineIndex + 1 < lines.length ? lines[lineIndex + 1] : null;

    // Compute line progress
    const lineDuration = Math.max(1, currentLine.endMs - currentLine.startMs);
    const lineProgress = clamp((positionMs - currentLine.startMs) / lineDuration, 0, 1);

    // Compute word progress if word-sync is available
    let wordIndex = -1;
    let wordProgress = 0;

    if (currentLine.words && currentLine.words.length > 0) {
        wordIndex = findActiveWordIndex(
            currentLine.words,
            positionMs,
            lineIndex === lastLineIndex ? lastWordIndex : -1,
        );

        if (wordIndex >= 0 && wordIndex < currentLine.words.length) {
            const word = currentLine.words[wordIndex];
            const wordDuration = Math.max(1, word.endMs - word.startMs);
            wordProgress = clamp((positionMs - word.startMs) / wordDuration, 0, 1);
        } else if (positionMs >= currentLine.endMs) {
            wordIndex = currentLine.words.length - 1;
            wordProgress = 1;
        } else if (positionMs < currentLine.words[0].startMs) {
            wordIndex = 0;
            wordProgress = 0;
        }
    }

    return {
        activeLineIndex: lineIndex,
        activeWordIndex: wordIndex,
        wordProgress,
        lineProgress,
        currentLine,
        nextLine,
    };
}
