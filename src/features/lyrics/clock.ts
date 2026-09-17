/**
 * High-precision Playback Clock for smooth frontend interpolation
 * Prevents IPC spam at 60 FPS while enabling buttery-smooth sub-frame rendering.
 */
export class PlaybackClock {
    private basePositionMs: number = 0;
    private receivedAt: number = 0;
    private playing: boolean = false;
    private playbackRate = 1;
    private driftToleranceMs: number = 100;

    constructor(driftToleranceMs = 100) {
        this.driftToleranceMs = driftToleranceMs;
        this.receivedAt = performance.now();
    }

    /**
     * Updates the clock snapshot from a backend media sync event
     */
    public updateSnapshot(positionMs: number, playing: boolean, forceReset = false, playbackRate = this.playbackRate): void {
        if (!Number.isFinite(positionMs)) return;
        const rate = Number.isFinite(playbackRate) && playbackRate > 0 ? playbackRate : 1;
        const now = performance.now();
        const currentExtrapolated = this.getPositionMs();
        const drift = Math.abs(currentExtrapolated - positionMs);
        const stateChanged = this.playing !== playing;

        if (forceReset || !playing || stateChanged || rate !== this.playbackRate || drift > this.driftToleranceMs) {
            this.basePositionMs = Math.max(0, positionMs);
            this.receivedAt = now;
            this.playing = playing;
        } else {
            // Keep playing state up to date without abruptly jumping timestamp
            this.playing = playing;
        }
        this.playbackRate = rate;
    }

    /**
     * Interpolates the current playback position in milliseconds
     */
    public getPositionMs(): number {
        if (!this.playing) {
            return this.basePositionMs;
        }
        const elapsed = performance.now() - this.receivedAt;
        return Math.max(0, this.basePositionMs + elapsed * this.playbackRate);
    }

    public isPlaying(): boolean {
        return this.playing;
    }

    public reset(positionMs = 0, playing = false): void {
        this.basePositionMs = Math.max(0, positionMs);
        this.receivedAt = performance.now();
        this.playing = playing;
        this.playbackRate = 1;
    }
}
