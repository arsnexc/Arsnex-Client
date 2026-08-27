package dev.arsex.module;

import java.util.List;
import java.util.function.Supplier;

/**
 * A single tunable on a Module. Settings are self-describing so the Click GUI
 * can render any module without knowing what it is.
 */
public abstract class Setting<T> {
    public final String name;
    public final String description;
    protected T value;
    /** Optional gate: setting only shows when this returns true. */
    public Supplier<Boolean> visibleWhen = () -> true;

    protected Setting(String name, String description, T def) {
        this.name = name;
        this.description = description;
        this.value = def;
    }

    public T get() { return value; }
    public void set(T v) { this.value = v; }
    public boolean visible() { return visibleWhen.get(); }

    public abstract String serialize();
    public abstract void deserialize(String raw);

    // ---- concrete types ----

    public static final class Bool extends Setting<Boolean> {
        public Bool(String n, String d, boolean def) { super(n, d, def); }
        public String serialize() { return Boolean.toString(value); }
        public void deserialize(String r) { value = Boolean.parseBoolean(r); }
    }

    public static final class Slider extends Setting<Double> {
        public final double min, max, step;
        public final String unit;
        public Slider(String n, String d, double def, double min, double max,
                      double step, String unit) {
            super(n, d, def);
            this.min = min; this.max = max; this.step = step; this.unit = unit;
        }
        @Override public void set(Double v) {
            double clamped = Math.max(min, Math.min(max, v));
            // Quantise to step so the UI and the config agree exactly.
            this.value = Math.round(clamped / step) * step;
        }
        public String serialize() { return Double.toString(value); }
        public void deserialize(String r) { set(Double.parseDouble(r)); }
    }

    public static final class Mode extends Setting<String> {
        public final List<String> options;
        public Mode(String n, String d, String def, String... opts) {
            super(n, d, def);
            this.options = List.of(opts);
        }
        @Override public void set(String v) {
            if (options.contains(v)) this.value = v;
        }
        public void cycle() {
            int i = options.indexOf(value);
            value = options.get((i + 1) % options.size());
        }
        public String serialize() { return value; }
        public void deserialize(String r) { set(r); }
    }
}
