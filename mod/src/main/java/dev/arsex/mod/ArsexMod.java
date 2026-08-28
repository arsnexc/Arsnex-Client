package dev.arsex.mod;

import dev.arsex.mod.config.ConfigIO;
import dev.arsex.mod.gui.ClickGui;
import dev.arsex.mod.hud.HudRenderer;
import dev.arsex.mod.module.ModuleManager;
import dev.arsex.mod.modules.Coordinates;
import dev.arsex.mod.modules.Cps;
import dev.arsex.mod.modules.FpsCounter;
import dev.arsex.mod.modules.Fullbright;
import dev.arsex.mod.modules.Zoom;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.DrawContext;
import org.lwjgl.glfw.GLFW;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.nio.file.Path;
import java.util.HashSet;
import java.util.Set;

/**
 * Fabric client entrypoint.
 *
 * Everything is static and nullable-guarded because mixins can fire before or
 * after initialisation depending on load order; a null check is cheaper and
 * safer than trying to guarantee ordering across mods.
 */
public final class ArsexMod implements ClientModInitializer {

    public static final Logger LOG = LoggerFactory.getLogger("Arsex");

    private static ModuleManager modules;
    private static HudRenderer hud;
    private static ConfigIO config;

    private static Fullbright fullbright;
    private static Zoom zoom;
    private static Cps cps;
    private static FpsCounter fps;
    private static Coordinates coords;

    /** Default GUI key: RIGHT SHIFT. */
    private static final int GUI_KEY = GLFW.GLFW_KEY_RIGHT_SHIFT;
    private static boolean guiKeyWasDown = false;

    /** Keys currently held, so a held bind fires once rather than every tick. */
    private static final Set<Integer> pressed = new HashSet<>();

    @Override
    public void onInitializeClient() {
        modules = new ModuleManager();
        hud = new HudRenderer();

        fullbright = new Fullbright();
        zoom       = new Zoom();
        cps        = new Cps();
        fps        = new FpsCounter();
        coords     = new Coordinates();

        modules.registerAll(fullbright, zoom, cps, fps, coords);

        Path dir = FabricLoader.getInstance().getConfigDir().resolve("arsex");
        config = new ConfigIO(dir.resolve("modules.json"));
        try {
            config.load(modules);
            LOG.info("loaded config from {}", dir.resolve("modules.json"));
        } catch (Exception e) {
            LOG.warn("could not load config, using defaults", e);
        }

        ClientTickEvents.END_CLIENT_TICK.register(ArsexMod::onTick);

        LOG.info("Arsex client initialised with {} modules", modules.all().size());
    }

    private static void onTick(MinecraftClient mc) {
        if (mc == null || modules == null) return;

        // Keybinds only while no screen is open, so typing in chat never toggles.
        if (mc.currentScreen == null && mc.getWindow() != null) {
            long w = mc.getWindow().getHandle();

            boolean down = GLFW.glfwGetKey(w, GUI_KEY) == GLFW.GLFW_PRESS;
            if (down && !guiKeyWasDown) mc.setScreen(new ClickGui());
            guiKeyWasDown = down;

            for (var m : modules.all()) {
                int k = m.getKeybind();
                if (k <= 0) continue;
                boolean isDown = GLFW.glfwGetKey(w, k) == GLFW.GLFW_PRESS;
                if (isDown && pressed.add(k)) {
                    modules.onKey(k);
                    saveConfig();
                } else if (!isDown) {
                    pressed.remove(k);
                }
            }
        }

        modules.onTick();

        // Fullbright needs a tick even while disabled to finish its fade-out.
        if (fullbright != null && !fullbright.isEnabled()) fullbright.tickRestore();
    }

    public static void renderHud(DrawContext ctx) {
        if (hud != null) hud.render(ctx);
    }

    public static void saveConfig() {
        if (config == null || modules == null) return;
        try {
            config.save(modules);
        } catch (Exception e) {
            LOG.warn("could not save config", e);
        }
    }

    public static ModuleManager modules() { return modules; }
    public static Fullbright fullbright() { return fullbright; }
    public static Zoom zoom()             { return zoom; }
    public static Cps cps()               { return cps; }
    public static FpsCounter fps()        { return fps; }
    public static Coordinates coords()    { return coords; }
}
