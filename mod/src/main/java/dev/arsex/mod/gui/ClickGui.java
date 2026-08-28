package dev.arsex.mod.gui;

import dev.arsex.mod.ArsexMod;
import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.Setting;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.text.Text;

import java.util.ArrayList;
import java.util.List;

/**
 * In-game module configuration menu. Opened with RIGHT SHIFT by default.
 *
 * Monochrome by construction — every colour constant is greyscale, matching
 * the launcher. Layout is chamfered rather than rounded, same as the launcher
 * shell: no border radius anywhere.
 */
public final class ClickGui extends Screen {

    private static final int BG      = 0xD8000000;
    private static final int PANEL   = 0xF0080808;
    private static final int HEADER  = 0xFF101010;
    private static final int PAPER   = 0xFFF5F5F5;
    private static final int MUTED   = 0xFF8C8C8C;
    private static final int EDGE    = 0x40FFFFFF;
    private static final int ON_FILL = 0xFFF5F5F5;

    private static final int COL_W = 118;
    private static final int ROW_H = 14;
    private static final int GAP   = 8;

    /** Which modules have their settings expanded, keyed by id. */
    private final List<String> expanded = new ArrayList<>();
    private Module awaitingBind;

    public ClickGui() {
        super(Text.literal("Arsex"));
    }

    @Override public boolean shouldPause() { return false; }   // never freeze singleplayer

    @Override
    public void render(DrawContext ctx, int mouseX, int mouseY, float delta) {
        ctx.fill(0, 0, width, height, BG);

        int x = GAP;
        for (Category cat : Category.values()) {
            int y = GAP;
            // Category header
            ctx.fill(x, y, x + COL_W, y + ROW_H, HEADER);
            edge(ctx, x, y, x + COL_W, y + ROW_H);
            ctx.drawText(textRenderer, cat.kanji + "  " + cat.label.toUpperCase(),
                    x + 5, y + 3, PAPER, false);
            y += ROW_H;

            for (Module m : ArsexMod.modules().byCategory(cat)) {
                boolean on = m.isEnabled();
                ctx.fill(x, y, x + COL_W, y + ROW_H, PANEL);
                if (on) ctx.fill(x, y, x + 2, y + ROW_H, ON_FILL);   // active bar
                edge(ctx, x, y, x + COL_W, y + ROW_H);

                ctx.drawText(textRenderer, m.name, x + 7, y + 3, on ? PAPER : MUTED, false);

                String right = (awaitingBind == m) ? "..."
                        : (m.getKeybind() > 0 ? keyName(m.getKeybind()) : "");
                if (!right.isEmpty()) {
                    ctx.drawText(textRenderer, right,
                            x + COL_W - textRenderer.getWidth(right) - 5, y + 3, MUTED, false);
                }
                y += ROW_H;

                if (expanded.contains(m.id)) {
                    for (Setting<?> s : m.settings()) {
                        if (!s.visible()) continue;
                        ctx.fill(x + 4, y, x + COL_W, y + ROW_H, 0xF0000000);
                        ctx.drawText(textRenderer, "  " + s.name, x + 7, y + 3, MUTED, false);
                        String v = valueLabel(s);
                        ctx.drawText(textRenderer, v,
                                x + COL_W - textRenderer.getWidth(v) - 5, y + 3, PAPER, false);
                        y += ROW_H;
                    }
                }
            }
            x += COL_W + GAP;
            if (x + COL_W > width) break;      // never draw off-screen
        }

        String hint = "LEFT CLICK TOGGLE   \u00b7   RIGHT CLICK SETTINGS   \u00b7   MIDDLE CLICK BIND   \u00b7   ESC CLOSE";
        ctx.drawText(textRenderer, hint,
                (width - textRenderer.getWidth(hint)) / 2, height - 14, MUTED, false);

        super.render(ctx, mouseX, mouseY, delta);
    }

    private String valueLabel(Setting<?> s) {
        if (s instanceof Setting.Bool b)   return b.get() ? "ON" : "OFF";
        if (s instanceof Setting.Slider sl) {
            double v = sl.get();
            String n = (v == Math.rint(v)) ? String.valueOf((long) v) : String.valueOf(v);
            return n + sl.unit;
        }
        if (s instanceof Setting.Mode m)   return m.get();
        return String.valueOf(s.get());
    }

    private void edge(DrawContext ctx, int x1, int y1, int x2, int y2) {
        ctx.fill(x1, y1, x2, y1 + 1, EDGE);
        ctx.fill(x1, y2 - 1, x2, y2, EDGE);
        ctx.fill(x1, y1, x1 + 1, y2, EDGE);
        ctx.fill(x2 - 1, y1, x2, y2, EDGE);
    }

    @Override
    public boolean mouseClicked(double mx, double my, int button) {
        Hit hit = hitTest(mx, my);
        if (hit == null) return super.mouseClicked(mx, my, button);

        if (hit.setting != null) {
            adjust(hit.setting, button);
            ArsexMod.saveConfig();
            return true;
        }
        if (hit.module != null) {
            switch (button) {
                case 0 -> { hit.module.toggle(); ArsexMod.saveConfig(); }
                case 1 -> {
                    if (!expanded.remove(hit.module.id)) expanded.add(hit.module.id);
                }
                case 2 -> awaitingBind = hit.module;
            }
            return true;
        }
        return super.mouseClicked(mx, my, button);
    }

    private void adjust(Setting<?> s, int button) {
        if (s instanceof Setting.Bool b) {
            b.set(!b.get());
        } else if (s instanceof Setting.Mode m) {
            m.cycle();
        } else if (s instanceof Setting.Slider sl) {
            // Left steps up, right steps down. A drag bar is nicer but a click
            // step is unambiguous and cannot produce an out-of-range value.
            double d = (button == 0 ? 1 : -1) * sl.step;
            sl.set(sl.get() + d);
        }
    }

    @Override
    public boolean keyPressed(int key, int scan, int mods) {
        if (awaitingBind != null) {
            // ESC clears a bind rather than closing, which is the least
            // surprising behaviour while explicitly in "press a key" mode.
            awaitingBind.setKeybind(key == 256 ? -1 : key);
            ArsexMod.modules().reindexKeys();
            ArsexMod.saveConfig();
            awaitingBind = null;
            return true;
        }
        return super.keyPressed(key, scan, mods);
    }

    @Override public void close() {
        ArsexMod.saveConfig();
        super.close();
    }

    // ---------------------------------------------------------------- hit test

    private record Hit(Module module, Setting<?> setting) {}

    /** Mirrors the render layout exactly; both must change together. */
    private Hit hitTest(double mx, double my) {
        int x = GAP;
        for (Category cat : Category.values()) {
            int y = GAP + ROW_H;
            if (mx >= x && mx < x + COL_W) {
                for (Module m : ArsexMod.modules().byCategory(cat)) {
                    if (my >= y && my < y + ROW_H) return new Hit(m, null);
                    y += ROW_H;
                    if (expanded.contains(m.id)) {
                        for (Setting<?> s : m.settings()) {
                            if (!s.visible()) continue;
                            if (my >= y && my < y + ROW_H) return new Hit(null, s);
                            y += ROW_H;
                        }
                    }
                }
            }
            x += COL_W + GAP;
            if (x + COL_W > width) break;
        }
        return null;
    }

    private static String keyName(int key) {
        if (key >= 65 && key <= 90) return String.valueOf((char) key);
        if (key >= 48 && key <= 57) return String.valueOf((char) key);
        return "#" + key;
    }
}
