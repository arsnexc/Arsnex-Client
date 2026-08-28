package dev.arsex.mod.module;

import java.util.List;
import java.util.function.Supplier;

/**
 * A tunable on a Module. Settings are self-describing so the Click GUI can
 * render any module without knowing what it is, and so config round-trips
 * through plain strings.
 */
public abstract class Setting<T> {
    public final String name;
    public final String description;
    protected T value;

    /** Optional gate: the row only renders when this returns true. */
    public Supplier<Boolean> visibleWhen = () -> true;

    protected Setting(String name, String description, T def) {
        this.name = name;
        this.description = description;
        this.value = def;
    }

    public T get()             { return value; }
    public void set(T v)       { this.value = v; }
    public boolean visible()   { return visibleWhen.get(); }

    public abstract String serialize();
    public abstract void deserialize(String raw);

    // ---------------------------------------------------------------- types

    public static final class Bool extends Setting<Boolean> {
        public Bool(String n, String d, boolean def) { super(n, d, def); }
        @Override public String serialize() { return Boolean.toString(value); }
        @Override public void deserialize(String r) { value = Boolean.parseBoolean(r); }
    }

    public static final class Slider extends Setting<Double> {
        public final double min, max, step;
        public final String unit;

        public Slider(String n, String d, double def, double min, double max, double step, String unit) {
            super(n, d, def);
            this.min = min; this.max = max; this.step = step; this.unit = unit;
        }

        /** Clamps and snaps to the step grid, so the GUI cannot produce junk. */
        @Override public void set(Double v) {
            double c = Math.max(min, Math.min(max, v));
            if (step > 0) c = min + Math.round((c - min) / step) * step;
            // Kill float drift so 0.1-steps do not render as 0.30000000000000004.
            value = Math.round(c * 1e6) / 1e6;
        }

        public float getFloat() { return value.floatValue(); }
        public int getInt()     { return (int) Math.round(value); }

        @Override public String serialize() { return Double.toString(value); }
        @Override public void deserialize(String r) {
            try { set(Double.parseDouble(r)); } catch (NumberFormatException ignored) {}
        }
    }

    public static final class Mode extends Setting<String> {
        public final List<String> options;

        public Mode(String n, String d, String def, List<String> options) {
            super(n, d, def);
            this.options = List.copyOf(options);
        }

        /** Ignores values not in the option list rather than accepting garbage. */
        @Override public void set(String v) { if (options.contains(v)) value = v; }

        public void cycle() {
            int i = options.indexOf(value);
            value = options.get((i + 1) % options.size());
        }

        public int index() { return Math.max(0, options.indexOf(value)); }

        @Override public String serialize() { return value; }
        @Override public void deserialize(String r) { set(r); }
    }
}
