package dev.arsex.mod.modules;

import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.Setting;

/**
 * Coordinate readout with nether/overworld conversion.
 *
 * The conversion is the part players actually want and the part that is easy
 * to get wrong, so it lives here as pure arithmetic and is unit-tested.
 */
public final class Coordinates extends Module {
    private final Setting.Bool netherConv = register(new Setting.Bool(
            "Nether Convert", "Show the matching coordinate in the other dimension.", true));

    private final Setting.Mode precision = register(new Setting.Mode(
            "Precision", "Decimal places.", "0", java.util.List.of("0", "1", "2")));

    public Coordinates() {
        super("coords", "Coordinates", "Position readout with nether conversion", Category.HUD);
    }

    public String format(double x, double y, double z) {
        int p = Integer.parseInt(precision.get());
        String f = "%." + p + "f";
        return String.format(f + ", " + f + ", " + f, x, y, z);
    }

    /** Overworld -> Nether is /8, Nether -> Overworld is *8. Y is unchanged. */
    public String convert(double x, double z, boolean inNether) {
        double cx = inNether ? x * 8 : x / 8;
        double cz = inNether ? z * 8 : z / 8;
        String label = inNether ? "OW" : "NE";
        return String.format("%s %.0f, %.0f", label, cx, cz);
    }

    public boolean showConversion() { return netherConv.get(); }
}
