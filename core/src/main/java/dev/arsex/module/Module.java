package dev.arsex.module;

import java.util.ArrayList;
import java.util.List;

/**
 * Base class for every Arsex module.
 *
 * Contract: modules are stateless with respect to the game between enable and
 * disable. onDisable() MUST fully revert anything onEnable() changed — this is
 * what makes hot-toggling safe and crash recovery able to restore state.
 */
public abstract class Module {
    public final String id;
    public final String name;
    public final String description;
    public final Category category;

    private boolean enabled;
    private int keybind;                 // GLFW key code, -1 = unbound
    private final List<Setting<?>> settings = new ArrayList<>();

    /** Set when the module is toggled; drives the GUI's slide-in animation. */
    public long lastToggleNanos;

    protected Module(String id, String name, String description, Category category) {
        this.id = id;
        this.name = name;
        this.description = description;
        this.category = category;
        this.keybind = -1;
    }

    protected <S extends Setting<?>> S register(S s) {
        settings.add(s);
        return s;
    }

    public List<Setting<?>> settings() { return settings; }

    public boolean isEnabled() { return enabled; }
    public int getKeybind() { return keybind; }
    public void setKeybind(int k) { this.keybind = k; }

    public void toggle() { setEnabled(!enabled); }

    public void setEnabled(boolean state) {
        if (this.enabled == state) return;
        this.enabled = state;
        this.lastToggleNanos = System.nanoTime();
        try {
            if (state) onEnable(); else onDisable();
        } catch (Throwable t) {
            // A broken module must never take down the game. Fail it closed.
            this.enabled = false;
            ModuleManager.LOG.error("Module '{}' threw on toggle; forced off", id, t);
        }
    }

    protected void onEnable() {}
    protected void onDisable() {}

    /** Called every client tick while enabled. */
    public void onTick() {}

    /** Called every frame while enabled. delta = partial tick. */
    public void onRender(float delta) {}

    /** Extra text shown after the name in the GUI, e.g. Zoom [4x]. */
    public String hudSuffix() { return null; }
}
