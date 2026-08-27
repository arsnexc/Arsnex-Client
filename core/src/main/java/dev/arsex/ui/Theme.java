package dev.arsex.ui;

/**
 * The monochrome lock, enforced in code.
 *
 * Every colour in the client comes from here. Colours are authored as a single
 * luminance byte plus alpha — it is structurally impossible to express a hue.
 * A unit test asserts R==G==B for every emitted value.
 */
public final class Theme {
    private Theme() {}

    public enum Variant {
        SUMI(1.00, 0xF5),    // ink — the default
        KOHAKU(0.92, 0xFA),  // paper — softer contrast, warmer white value
        TETSU(1.08, 0xEE),   // steel — punchier
        YAMI(1.18, 0xE8);    // void — maximum contrast

        public final double contrast;
        public final int paperLum;
        Variant(double contrast, int paperLum) {
            this.contrast = contrast;
            this.paperLum = paperLum;
        }
    }

    // NOTE: state fields MUST be declared before the constants below.
    // Java initialises static fields in source order, so lum() would read a
    // null `variant` if these came after. Found by the test harness.
    private static Variant variant = Variant.SUMI;
    private static boolean highContrast = false;

    // Luminance ladder. Names match the launcher's CSS tokens.
    public static final int INK_000 = lum(0x00);
    public static final int INK_050 = lum(0x05);
    public static final int INK_080 = lum(0x0A);
    public static final int INK_100 = lum(0x0E);
    public static final int INK_1C0 = lum(0x1C);
    public static final int INK_260 = lum(0x26);
    public static final int INK_3A0 = lum(0x3A);
    public static final int INK_5A0 = lum(0x5A);
    public static final int INK_8C0 = lum(0x8C);
    public static final int PAPER   = lum(0xF5);
    public static final int STEEL   = lum(0xFF);


    public static void setVariant(Variant v) { variant = v; }
    public static void setHighContrast(boolean b) { highContrast = b; }
    public static Variant variant() { return variant; }

    /** Build an opaque grey. The ONLY colour constructor in the codebase. */
    public static int lum(int l) { return lum(l, 0xFF); }

    public static int lum(int l, int alpha) {
        double c = variant.contrast * (highContrast ? 1.25 : 1.0);
        // Curve around mid-grey so both ends stay in range.
        int adj = (int) Math.round(128 + (l - 128) * c);
        int v = Math.max(0, Math.min(255, adj));
        return (alpha & 0xFF) << 24 | v << 16 | v << 8 | v;
    }

    /** Alpha-adjust an existing theme colour without touching luminance. */
    public static int alpha(int argb, double a) {
        int al = (int) Math.max(0, Math.min(255, Math.round(255 * a)));
        return (al << 24) | (argb & 0x00FFFFFF);
    }

    /** Assert-in-production guard used by the render layer. */
    public static boolean isMonochrome(int argb) {
        int r = (argb >> 16) & 0xFF, g = (argb >> 8) & 0xFF, b = argb & 0xFF;
        return r == g && g == b;
    }
}
