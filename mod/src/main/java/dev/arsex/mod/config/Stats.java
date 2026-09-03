package dev.arsex.mod.config;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;

/**
 * Live session stats written for the launcher to read.
 *
 * The launcher cannot see inside the game process, so the mod reports the
 * one number only it knows: real frame rate. Every rendered frame is fed to
 * {@link #frame(long)}; every two seconds the rolling average and maximum
 * are written to {@code config/arsex/stats.json} via temp file + atomic
 * move, with a wall-clock timestamp the launcher checks for freshness.
 *
 * Plain Java on purpose: no Fabric types, so the offline harness tests the
 * exact class that ships.
 */
public final class Stats {

    /** Frame-time samples kept for the average (~2s at 60fps, ~1s at 120). */
    private static final int RING = 120;
    /** Milliseconds between file writes. */
    private static final long FLUSH_EVERY_MS = 2000;

    private final Path file;
    private final double[] ms = new double[RING];
    private int idx = 0, filled = 0;
    private long lastNanos = 0;
    private long lastFlush = 0;

    public Stats(Path file) {
        this.file = file;
    }

    /** Call once per rendered frame with System.nanoTime(). */
    public synchronized void frame(long nowNanos) {
        if (lastNanos != 0) {
            double d = (nowNanos - lastNanos) / 1_000_000.0;
            // Ignore absurd deltas (alt-tab, breakpoint pauses) so a single
            // hitch cannot poison the average.
            if (d > 0 && d < 1000) {
                ms[idx] = d;
                idx = (idx + 1) % RING;
                if (filled < RING) filled++;
            }
        }
        lastNanos = nowNanos;
    }

    /** Average frames per second across the ring, 0 when empty. */
    public synchronized int fpsAvg() {
        double sum = 0;
        for (int i = 0; i < filled; i++) sum += ms[i];
        if (filled == 0 || sum <= 0) return 0;
        return (int) Math.round(1000.0 * filled / sum);
    }

    /** Best momentary rate in the ring (fastest frame), 0 when empty. */
    public synchronized int fpsMax() {
        double best = Double.MAX_VALUE;
        for (int i = 0; i < filled; i++) if (ms[i] < best) best = ms[i];
        return best == Double.MAX_VALUE || best <= 0 ? 0 : (int) Math.round(1000.0 / best);
    }

    /** Write the file if the interval elapsed. Called every frame; cheap. */
    public synchronized void maybeFlush(long nowMs) {
        if (nowMs - lastFlush < FLUSH_EVERY_MS) return;
        lastFlush = nowMs;
        try {
            Files.createDirectories(file.getParent());
            Path tmp = file.resolveSibling(file.getFileName() + ".tmp");
            Files.write(tmp, json(nowMs).getBytes(StandardCharsets.UTF_8));
            try {
                Files.move(tmp, file, StandardCopyOption.ATOMIC_MOVE,
                        StandardCopyOption.REPLACE_EXISTING);
            } catch (java.nio.file.AtomicMoveNotSupportedException e) {
                Files.move(tmp, file, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (IOException e) {
            // Stats are best-effort; a failure here must never touch gameplay.
        }
    }

    /** The JSON body, also the harness's assertion target. */
    synchronized String json(long nowMs) {
        return "{\"fpsAvg\":" + fpsAvg() + ",\"fpsMax\":" + fpsMax()
                + ",\"t\":" + nowMs + "}\n";
    }
}
