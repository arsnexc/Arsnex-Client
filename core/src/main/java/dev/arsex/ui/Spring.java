package dev.arsex.ui;

/**
 * Critically-dampable spring integrator.
 *
 * Why springs instead of duration-based easing: easing curves restart from zero
 * when interrupted, which produces a visible hitch if a user toggles something
 * mid-animation. A spring carries its velocity across retargets, so interrupted
 * motion stays continuous. This is the single biggest reason premium UIs feel
 * "alive" and cheap ones feel "scripted".
 *
 * Integrated with a fixed 1/240s substep so behaviour is identical at 60, 144
 * and 240 fps. Variable-dt spring integration is a classic source of
 * framerate-dependent overshoot.
 */
public final class Spring {
    private static final double SUBSTEP = 1.0 / 240.0;
    private static final double MAX_FRAME = 0.1; // clamp after alt-tab stalls

    private double value, target, velocity;
    private double prevValue;
    /** Interpolated render value. This is what callers should read. */
    private double rendered;
    private double stiffness, damping;
    private double accumulator;

    /** Global multiplier from the "Animation Speed" slider (0.0 - 2.0). */
    public static double speedScale = 1.0;
    /** Accessibility: reduced motion snaps everything instantly. */
    public static boolean reducedMotion = false;

    public Spring(double initial, double stiffness, double damping) {
        this.value = this.prevValue = this.rendered = initial;
        this.target = initial;
        this.stiffness = stiffness;
        this.damping = damping;
    }

    /** House default: snappy, ~4% overshoot. Matches --e-snap in the launcher. */
    public static Spring snappy(double initial) {
        return new Spring(initial, 380.0, 26.0);
    }

    /** No overshoot — for page transitions and anything that must not bounce. */
    public static Spring smooth(double initial) {
        return new Spring(initial, 210.0, 30.0);
    }

    /** Heavy, deliberate. Used by the boot blade and launch sequence. */
    public static Spring cinematic(double initial) {
        return new Spring(initial, 90.0, 19.0);
    }

    public Spring target(double t) { this.target = t; return this; }
    public double target() { return target; }
    /** Interpolated value for this frame. Framerate-independent. */
    public double value() { return rendered; }
    /** Raw simulation state, ignoring substep interpolation. Rarely needed. */
    public double rawValue() { return value; }
    public double velocity() { return velocity; }

    public void snap(double v) {
        this.value = this.prevValue = this.rendered = this.target = v;
        this.velocity = 0;
        this.accumulator = 0;
    }

    public boolean atRest() {
        return Math.abs(value - target) < 1e-3 && Math.abs(velocity) < 1e-3;
    }

    /** Total simulated time is what matters; see update()'s substep loop. */

    /** Advance by real frame time in seconds. */
    public double update(double dt) {
        if (reducedMotion) { snap(target); return rendered; }
        if (speedScale <= 0.0) return rendered;

        dt = Math.min(dt, MAX_FRAME) * speedScale;
        accumulator += dt;

        while (accumulator >= SUBSTEP) {
            prevValue = value;
            double force = -stiffness * (value - target) - damping * velocity;
            velocity += force * SUBSTEP;
            value += velocity * SUBSTEP;
            accumulator -= SUBSTEP;
        }

        if (atRest()) {
            value = prevValue = rendered = target;
            velocity = 0; accumulator = 0;
            return rendered;
        }

        // Interpolate across the leftover accumulator. Without this the sampled
        // value drifts by up to one substep depending on framerate, which the
        // test harness measured as a 0.029 spread between 60 and 240 fps.
        double alpha = accumulator / SUBSTEP;
        rendered = prevValue + (value - prevValue) * alpha;
        return rendered;
    }
}
