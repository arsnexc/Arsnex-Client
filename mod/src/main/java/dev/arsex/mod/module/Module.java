package dev.arsex.mod.module;

import java.util.ArrayList;
import java.util.List;

/**
 * Base class for every in-game Arsex module.
 *
 * Contract: onDisable() MUST fully revert whatever onEnable() changed. That is
 * what makes hot-toggling safe and lets the config restore state on load
 * without the game having to restart.
 *
 * Modules never touch Minecraft classes directly in this file — the concrete
 * subclasses do. Keeping the base free of MC types is what allows the whole
 * registry to be unit-tested without a game instance.
 */
public abstract class Module {
    public final String id;
    public final String name;
    public final String description;
    public final Category category;

    private boolean enabled;
    private int keybind = -1;              // GLFW key code, -1 = unbound
    private final List<Setting<?>> settings = new ArrayList<>();

    /** Set on every toggle; drives the GUI slide animation. */
    public long lastToggleNanos;

    protected Module(String id, String name, String description, Category category) {
        this.id = id;
        this.name = name;
        this.description = description;
        this.category = category;
    }

    protected <S extends Setting<?>> S register(S s) {
        settings.add(s);
        return s;
    }

    public List<Setting<?>> settings() { return settings; }
    public boolean isEnabled()         { return enabled; }
    public int getKeybind()            { return keybind; }
    public void setKeybind(int k)      { this.keybind = k; }
    public void toggle()               { setEnabled(!enabled); }

    public void setEnabled(boolean state) {
        if (this.enabled == state) return;
        this.enabled = state;
        this.lastToggleNanos = System.nanoTime();
        try {
            if (state) onEnable(); else onDisable();
        } catch (Throwable t) {
            // A broken module must never take the game down with it.
            this.enabled = false;
            ModuleManager.LOG.error("module {} failed to {}", id, state ? "enable" : "disable", t);
        }
    }

    /** Called on the client tick while enabled. Default: nothing. */
    public void onTick() {}

    protected void onEnable()  {}
    protected void onDisable() {}
}
