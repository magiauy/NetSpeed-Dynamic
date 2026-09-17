import type {
    LyricFrameState,
    NormalizedLyricLine,
    NormalizedLyricWord,
    NormalizedLyrics,
} from './types';

function clamp(value: number, min: number, max: number): number {
    return Math.min(Math.max(value, min), max);
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
    if (positionMs < lines[0].startMs) return -1;

    const total = lines.length;

    // Fast-path 1: Same line as previous frame
    if (hintIndex >= 0 && hintIndex < total) {
        const current = lines[hintIndex];
        if (positionMs >= current.startMs && positionMs < current.endMs
            && (hintIndex + 1 === total || positionMs < lines[hintIndex + 1].startMs)) {
            return hintIndex;
        }

        // Fast-path 2: Advance to next sequential line
        if (hintIndex + 1 < total) {
            const next = lines[hintIndex + 1];
            if (positionMs >= next.startMs && positionMs < next.endMs
                && (hintIndex + 2 === total || positionMs < lines[hintIndex + 2].startMs)) {
                return hintIndex + 1;
            }
        }
    }

    // Binary search: find largest index where lines[i].startMs <= positionMs
    let low = 0;
    let high = total - 1;
    let result = -1;

    while (low <= high) {
        const mid = (low + high) >> 1;
        if (lines[mid].startMs <= positionMs) {
            result = mid;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }

    // Earlier overlapping lines may still be active after a shorter cue ends.
    while (result >= 0 && positionMs >= lines[result].endMs) result--;
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
            nextLine: lines.find(line => line.startMs > positionMs) || null,
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
