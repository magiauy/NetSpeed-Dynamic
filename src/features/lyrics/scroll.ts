export function lyricScrollOffset(textWidth: number, viewportWidth: number, progress: number, durationMs = 4000): number {
    const overflow = Math.max(0, textWidth - viewportWidth);
    const duration = Math.max(1, durationMs);
    // Brief opening hold, with all pauses/ramps included in the cue's time budget.
    const holdMs = Math.min(250, duration * 0.08);
    const travelMs = duration - holdMs - Math.min(350, duration * 0.15);
    const elapsed = Math.min(travelMs, Math.max(0, progress * duration - holdMs));
    // Integrate a trapezoidal velocity profile: short ramps, constant cruise speed.
    // Normalize by the total area so even long/short cues reach the exact endpoint.
    const rampMs = Math.min(450, travelMs * 0.15);
    const area = travelMs - rampMs;
    let distance: number;
    if (elapsed < rampMs) {
        distance = elapsed * elapsed / (2 * rampMs);
    } else if (elapsed > travelMs - rampMs) {
        const remaining = travelMs - elapsed;
        distance = area - remaining * remaining / (2 * rampMs);
    } else {
        distance = elapsed - rampMs / 2;
    }
    return overflow * Math.min(1, Math.max(0, distance / area));
}

/** Monotone cubic interpolation of measured word positions and their actual times.
 * Shared tangents keep velocity continuous across words, including timing gaps.
 */
export function wordScrollOffset(
    textWidth: number, viewportWidth: number, positionMs: number,
    startMs: number, endMs: number, words: { startMs: number; left: number }[],
): number {
    const overflow = Math.max(0, textWidth - viewportWidth);
    if (!overflow) return 0;
    const duration = Math.max(1, endMs - startMs);
    const deadline = endMs - Math.min(200, duration * 0.1);
    const anchors = [{ time: startMs, offset: 0 }];
    for (const word of words) {
        if (word.startMs <= startMs || word.startMs >= deadline) continue;
        const previous = anchors[anchors.length - 1];
        // Keep the sung word in the readable right half until the tail fits.
        const offset = Math.max(previous.offset, Math.min(overflow, word.left - viewportWidth * 0.55));
        if (word.startMs === previous.time) previous.offset = offset;
        else if (word.startMs > previous.time) anchors.push({ time: word.startMs, offset });
    }
    anchors.push({ time: deadline, offset: overflow });
    if (positionMs <= startMs) return 0;
    if (positionMs >= deadline) return overflow;
    const slopes = anchors.slice(1).map((point, i) =>
        (point.offset - anchors[i].offset) / (point.time - anchors[i].time));
    const tangent = (i: number) => {
        if (i === 0 || i === anchors.length - 1) return 0;
        const a = slopes[i - 1], b = slopes[i];
        return a > 0 && b > 0 ? 2 * a * b / (a + b) : 0;
    };
    const i = anchors.findIndex((point, index) => index > 0 && point.time > positionMs) - 1;
    const a = anchors[i], b = anchors[i + 1];
    const span = b.time - a.time;
    const t = (positionMs - a.time) / span;
    const t2 = t * t, t3 = t2 * t;
    const offset = (2 * t3 - 3 * t2 + 1) * a.offset
        + (t3 - 2 * t2 + t) * span * tangent(i)
        + (-2 * t3 + 3 * t2) * b.offset
        + (t3 - t2) * span * tangent(i + 1);
    return Math.max(a.offset, Math.min(b.offset, offset));
}
