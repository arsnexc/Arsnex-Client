package dev.arsex.ui;

/**
 * Duration-based curves, for the cases where a spring is wrong: anything that
 * must finish at an exact time (boot sequence, timed HUD fades).
 * These mirror the launcher's CSS tokens 1:1 so in-game and launcher motion
 * are indistinguishable.
 */
public final class Ease {
    private Ease() {}

    /** cubic-bezier(.16,1,.3,1) — the house curve. */
    public static double expoOut(double t) {
        t = clamp(t);
        return t >= 1.0 ? 1.0 : 1.0 - Math.pow(2.0, -10.0 * t);
    }

    /** cubic-bezier(.65,0,.35,1) — symmetric. */
    public static double inOut(double t) {
        t = clamp(t);
        return t < 0.5
                ? 4.0 * t * t * t
                : 1.0 - Math.pow(-2.0 * t + 2.0, 3.0) / 2.0;
    }

    /** Quartic out — used by the launcher's stat count-up. */
    public static double quartOut(double t) {
        t = clamp(t);
        return 1.0 - Math.pow(1.0 - t, 4.0);
    }

    public static double lerp(double a, double b, double t) {
        return a + (b - a) * clamp(t);
    }

    /** Framerate-independent exponential smoothing. */
    public static double damp(double a, double b, double lambda, double dt) {
        return lerp(a, b, 1.0 - Math.exp(-lambda * dt));
    }

    private static double clamp(double t) {
        return t < 0 ? 0 : (t > 1 ? 1 : t);
    }
}
