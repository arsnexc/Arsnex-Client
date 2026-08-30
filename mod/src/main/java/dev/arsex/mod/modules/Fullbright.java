package dev.arsex.mod.modules;

import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.Setting;
import net.minecraft.client.MinecraftClient;

/**
 * Uniform luminance.
 *
 * Implemented by driving the vanilla gamma option rather than by patching the
 * lightmap: gamma is a float the game already respects everywhere, so this
 * works with shaders, in water, and in the nether without special cases.
 *
 * The original gamma is captured on enable and restored on disable, so
 * toggling never leaves the user's video settings modified.
 */
public final class Fullbright extends Module {
    private final Setting.Slider level = register(new Setting.Slider(
            "Level", "Target gamma. 1.0 is vanilla maximum; higher is brighter.",
            10.0, 1.0, 16.0, 0.5, ""));

    private final Setting.Bool smooth = register(new Setting.Bool(
            "Fade", "Ease gamma in and out instead of snapping.", true));

    private Double saved;      // null = not currently applied
    private double current;

    public Fullbright() {
        super("fullbright", "Fullbright", "Uniform luminance, no gamma clipping", Category.VISUAL);
        setKeybind(66); // B
    }

    @Override protected void onEnable() {
        MinecraftClient mc = MinecraftClient.getInstance();
        if (mc == null) return;
        // Clamp on capture: if a previous session crashed with fullbright on,
        // options.txt may hold a gamma above 1.0 and "restoring" it would
        // silently leave the game brighter than the user ever asked for.
        double now = Math.min(1.0, mc.options.getGamma().getValue());
        if (saved == null) saved = now;
        current = now;
        if (!smooth.get()) apply(level.get());
    }

    @Override protected void onDisable() {
        MinecraftClient mc = MinecraftClient.getInstance();
        if (mc == null || saved == null) return;
        if (!smooth.get()) {
            apply(saved);
            saved = null;
        }
        // When fading, tickRestore() finishes the restore and clears `saved`.
    }

    @Override public void onTick() {
        if (!smooth.get()) { apply(level.get()); return; }
        current = current + (level.get() - current) * 0.18;
        if (Math.abs(level.get() - current) < 1e-3) current = level.get();
        apply(current);
    }

    /**
     * Runs even while disabled so a fade-out can complete. Returns true once
     * the restore has finished and the module can be left alone.
     */
    public boolean tickRestore() {
        if (saved == null) return true;
        if (!smooth.get()) { apply(saved); saved = null; return true; }
        current = current + (saved - current) * 0.18;
        if (Math.abs(saved - current) < 1e-3) {
            apply(saved);
            saved = null;
            return true;
        }
        apply(current);
        return false;
    }

    private void apply(double g) {
        MinecraftClient mc = MinecraftClient.getInstance();
        if (mc == null) return;
        var option = mc.options.getGamma();
        if (g >= 0.0 && g <= 1.0) {
            // Inside the slider range, go through the front door.
            option.setValue(g);
            return;
        }
        // Above 1.0 the option's callbacks reject setValue() outright —
        // DoubleSliderCallbacks.validate() empties the Optional and setValue
        // keeps the old value. Write the backing field instead; gamma has no
        // dependent state, the lightmap polls it every update.
        ((dev.arsex.mod.mixin.SimpleOptionAccessor<Double>) (Object) option)
                .arsex$setValueDirect(g);
    }
}
