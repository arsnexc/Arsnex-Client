package dev.arsex.mod.hud;

import dev.arsex.mod.ArsexMod;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.DrawContext;
import net.minecraft.util.math.MathHelper;

import java.util.ArrayList;
import java.util.List;

/**
 * Monochrome HUD overlay.
 *
 * Strictly black/white/grey: the client's whole identity is the absence of
 * colour, so every ARGB constant here has equal R, G and B channels.
 */
public final class HudRenderer {

    // Greyscale palette. R==G==B in every value.
    public static final int INK   = 0xC0000000;   // panel backing
    public static final int PAPER = 0xFFF5F5F5;   // primary text
    public static final int MUTED = 0xFF8C8C8C;   // secondary text
    public static final int EDGE  = 0x40FFFFFF;   // hairline

    private static final int PAD = 4;
    private static final int LINE = 10;

    public void render(DrawContext ctx) {
        MinecraftClient mc = MinecraftClient.getInstance();
        if (mc == null || mc.player == null) return;
        if (mc.options.hudHidden) return;         // respect F1

        List<String> lines = collect(mc);
        if (lines.isEmpty()) return;

        int width = 0;
        for (String s : lines) width = Math.max(width, mc.textRenderer.getWidth(s));

        int x = 6, y = 6;
        int boxW = width + PAD * 2;
        int boxH = lines.size() * LINE + PAD * 2 - 2;

        ctx.fill(x, y, x + boxW, y + boxH, INK);
        // Hairline edge, drawn as four 1px fills so it stays crisp at any GUI scale.
        ctx.fill(x, y, x + boxW, y + 1, EDGE);
        ctx.fill(x, y + boxH - 1, x + boxW, y + boxH, EDGE);
        ctx.fill(x, y, x + 1, y + boxH, EDGE);
        ctx.fill(x + boxW - 1, y, x + boxW, y + boxH, EDGE);

        int ty = y + PAD;
        for (String s : lines) {
            ctx.drawText(mc.textRenderer, s, x + PAD, ty, PAPER, false);
            ty += LINE;
        }
    }

    /** Builds the readout. */
    private List<String> collect(MinecraftClient mc) {
        List<String> out = new ArrayList<>();

        var fps = ArsexMod.fps();
        if (fps != null && fps.isEnabled()) {
            fps.frame();
            out.add(fps.display());
        }

        var cps = ArsexMod.cps();
        if (cps != null && cps.isEnabled()) out.add("CPS " + cps.display());

        var coords = ArsexMod.coords();
        if (coords != null && coords.isEnabled() && mc.player != null) {
            double x = mc.player.getX(), y = mc.player.getY(), z = mc.player.getZ();
            out.add("XYZ " + coords.format(x, y, z));
            if (coords.showConversion()) {
                boolean nether = mc.world != null
                        && mc.world.getRegistryKey().getValue().getPath().contains("nether");
                out.add(coords.convert(x, z, nether));
            }
            out.add(facing(mc.player.getYaw()));
        }
        return out;
    }

    /** Compass direction plus the axis sign players actually navigate by. */
    public static String facing(float yaw) {
        int i = MathHelper.floor((yaw * 4.0F / 360.0F) + 0.5D) & 3;
        return switch (i) {
            case 0 -> "S  (+Z)";
            case 1 -> "W  (-X)";
            case 2 -> "N  (-Z)";
            default -> "E  (+X)";
        };
    }
}
