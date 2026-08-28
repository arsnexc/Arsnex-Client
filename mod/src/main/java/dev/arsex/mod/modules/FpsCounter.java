package dev.arsex.mod.modules;

import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.Setting;

/**
 * Frame counter with a rolling 1% low.
 *
 * Vanilla's debug FPS is an integer averaged over a whole second, which hides
 * exactly the stutter players care about. This keeps a ring of recent frame
 * times so it can report the 1% low, which is the number that correlates with
 * perceived smoothness.
 */
public final class FpsCounter extends Module {
    private final Setting.Bool showLow = register(new Setting.Bool(
            "1% Low", "Also show the 1% low frame rate.", true));

    private static final int RING = 240;           // ~2s at 120fps
    private final double[] frameMs = new double[RING];
    private int idx = 0, filled = 0;
    private long lastNanos = 0;

    public FpsCounter() {
        super("fps", "FPS Counter", "Frame rate with rolling 1% low", Category.HUD);
    }

    /** Call once per rendered frame. */
    public void frame() {
        long now = System.nanoTime();
        if (lastNanos != 0) {
            double ms = (now - lastNanos) / 1_000_000.0;
            // Ignore absurd deltas (alt-tab, breakpoint) so they cannot poison the low.
            if (ms > 0 && ms < 1000) {
                frameMs[idx] = ms;
                idx = (idx + 1) % RING;
                if (filled < RING) filled++;
            }
        }
        lastNanos = now;
    }

    public int fps() {
        if (filled == 0) return 0;
        double sum = 0;
        for (int i = 0; i < filled; i++) sum += frameMs[i];
        double avg = sum / filled;
        return avg <= 0 ? 0 : (int) Math.round(1000.0 / avg);
    }

    /** The slowest 1% of frames, expressed as an fps figure. */
    public int onePercentLow() {
        if (filled < 10) return fps();
        double[] copy = new double[filled];
        System.arraycopy(frameMs, 0, copy, 0, filled);
        java.util.Arrays.sort(copy);
        int n = Math.max(1, filled / 100);
        double sum = 0;
        for (int i = 0; i < n; i++) sum += copy[filled - 1 - i];   // slowest frames
        double avg = sum / n;
        return avg <= 0 ? 0 : (int) Math.round(1000.0 / avg);
    }

    public String display() {
        return showLow.get() ? fps() + " FPS · " + onePercentLow() + " LOW" : fps() + " FPS";
    }

    @Override protected void onDisable() {
        idx = 0; filled = 0; lastNanos = 0;
    }
}
