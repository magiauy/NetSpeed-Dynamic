export function lyricScrollOffset(textWidth: number, viewportWidth: number, progress: number): number {
    const overflow = Math.max(0, textWidth - viewportWidth);
    // Hold briefly at either end; ease into/out of motion without word-sized jumps.
    const t = Math.min(1, Math.max(0, (progress - 0.1) / 0.75));
    return overflow * t * t * (3 - 2 * t);
}
