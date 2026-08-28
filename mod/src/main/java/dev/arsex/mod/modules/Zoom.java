package dev.arsex.mod.modules;

import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.Setting;

/**
 * Cinematic zoom.
 *
 * The FOV multiplier is read by a mixin on GameRenderer#getFov, so this class
 * only owns the easing state. Keeping the maths here (and free of Minecraft
 * types) means the easing curve is unit-testable without a game.
 */
public final class Zoom extends Module {
    private final Setting.Slider factor = register(new Setting.Slider(
            "Factor", "How far to zoom. 4x means FOV/4.",
            4.0, 1.5, 12.0, 0.5, "x"));

    private final Setting.Bool smooth = register(new Setting.Bool(
            "Smooth", "Ease the transition instead of snapping.", true));

    private final Setting.Slider speed = register(new Setting.Slider(
            "Speed", "Easing rate. Higher settles faster.",
            0.28, 0.05, 1.0, 0.01, ""));

    /** 0.0 = no zoom, 1.0 = fully zoomed. */
    private double progress = 0.0;

    public Zoom() {
        super("zoom", "Zoom", "Cinematic eased zoom with inertia", Category.VISUAL);
        setKeybind(67); // C
    }

    /** Advances the easing. Called every frame, including while disabled. */
    public void advance() {
        double target = isEnabled() ? 1.0 : 0.0;
        if (!smooth.get()) { progress = target; return; }
        progress += (target - progress) * speed.get();
        if (Math.abs(target - progress) < 1e-4) progress = target;
    }

    /** Multiplier applied to the vanilla FOV. 1.0 when fully zoomed out. */
    public double fovMultiplier() {
        double full = 1.0 / factor.get();
        return 1.0 + (full - 1.0) * progress;
    }

    public boolean isZooming() { return progress > 1e-4; }

    /** Mouse sensitivity should scale with zoom or aiming becomes unusable. */
    public double sensitivityMultiplier() {
        return isZooming() ? Math.max(0.15, fovMultiplier()) : 1.0;
    }
}
