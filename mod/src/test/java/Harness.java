import dev.arsex.mod.config.Stats;
import dev.arsex.mod.config.ConfigIO;
import dev.arsex.mod.module.Category;
import dev.arsex.mod.module.Module;
import dev.arsex.mod.module.ModuleManager;
import dev.arsex.mod.module.Setting;
import dev.arsex.mod.modules.Coordinates;
import dev.arsex.mod.modules.Cps;
import dev.arsex.mod.modules.FpsCounter;
import dev.arsex.mod.modules.Zoom;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * Dependency-free test run for everything in the mod that does not need a
 * live Minecraft instance: the module system, settings, config round-trip and
 * the four pure-logic modules.
 */
public final class Harness {

    static int pass = 0, fail = 0;

    static void check(String name, boolean ok) {
        if (ok) { pass++; System.out.println("  PASS  " + name); }
        else    { fail++; System.out.println("  FAIL  " + name); }
    }

    static void eq(String name, Object a, Object b) {
        boolean ok = (a == null) ? b == null : a.equals(b);
        if (!ok) System.out.println("        expected <" + b + "> got <" + a + ">");
        check(name, ok);
    }

    // A minimal module used to exercise the base contract.
    static final class Probe extends Module {
        int enables = 0, disables = 0, ticks = 0;
        boolean throwOnEnable = false;
        final Setting.Bool flag = register(new Setting.Bool("Flag", "", false));
        final Setting.Slider num = register(new Setting.Slider("Num", "", 5, 0, 10, 0.5, "%"));
        final Setting.Mode mode  = register(new Setting.Mode("Mode", "", "A", List.of("A", "B", "C")));

        Probe(String id) { super(id, id, "probe", Category.UTILITY); }

        @Override protected void onEnable()  { enables++; if (throwOnEnable) throw new RuntimeException("boom"); }
        @Override protected void onDisable() { disables++; }
        @Override public void onTick()       { ticks++; }
    }

    public static void main(String[] args) throws Exception {
        System.out.println("Arsex mod core");

        // ------------------------------------------------------ module basics
        System.out.println("\nModule");
        Probe p = new Probe("probe");
        check("starts disabled", !p.isEnabled());
        p.toggle();
        check("toggle enables", p.isEnabled());
        eq("onEnable fired once", p.enables, 1);
        p.toggle();
        check("toggle disables", !p.isEnabled());
        eq("onDisable fired once", p.disables, 1);
        p.setEnabled(false);
        eq("redundant setEnabled is a no-op", p.disables, 1);
        check("lastToggleNanos set", p.lastToggleNanos > 0);

        Probe bad = new Probe("bad");
        bad.throwOnEnable = true;
        bad.setEnabled(true);
        check("throwing onEnable leaves module disabled", !bad.isEnabled());

        // ---------------------------------------------------------- settings
        System.out.println("\nSetting");
        eq("bool default", p.flag.get(), false);
        p.flag.set(true);
        eq("bool serialize", p.flag.serialize(), "true");
        p.flag.deserialize("false");
        eq("bool deserialize", p.flag.get(), false);

        p.num.set(99.0);
        eq("slider clamps to max", p.num.get(), 10.0);
        p.num.set(-5.0);
        eq("slider clamps to min", p.num.get(), 0.0);
        p.num.set(3.3);
        eq("slider snaps to step", p.num.get(), 3.5);
        p.num.set(2.5);
        eq("slider int accessor", p.num.getInt(), 3);   // 2.5 rounds half-up

        p.mode.set("B");
        eq("mode set", p.mode.get(), "B");
        p.mode.set("Z");
        eq("mode rejects unknown option", p.mode.get(), "B");
        p.mode.cycle();
        eq("mode cycles", p.mode.get(), "C");
        p.mode.cycle();
        eq("mode wraps", p.mode.get(), "A");

        Setting.Bool gated = new Setting.Bool("Gated", "", true);
        gated.visibleWhen = () -> p.flag.get();
        p.flag.set(false);
        check("visibleWhen hides", !gated.visible());
        p.flag.set(true);
        check("visibleWhen shows", gated.visible());

        // ----------------------------------------------------------- manager
        System.out.println("\nModuleManager");
        ModuleManager mm = new ModuleManager();
        Probe a = new Probe("alpha"), b = new Probe("bravo");
        mm.registerAll(a, b);
        eq("registered count", mm.all().size(), 2);
        check("get by id", mm.get("alpha").isPresent());
        check("get unknown is empty", mm.get("nope").isEmpty());

        boolean dup = false;
        try { mm.register(new Probe("alpha")); } catch (IllegalStateException e) { dup = true; }
        check("duplicate id rejected", dup);

        a.setKeybind(65);
        mm.reindexKeys();
        eq("keybind fires one module", mm.onKey(65), 1);
        check("keybind toggled it", a.isEnabled());
        eq("unbound key fires nothing", mm.onKey(99), 0);

        b.setKeybind(65);
        mm.reindexKeys();
        eq("shared keybind fires both", mm.onKey(65), 2);

        eq("enabled list", mm.enabled().size(), 1);   // a off, b on
        eq("byCategory", mm.byCategory(Category.UTILITY).size(), 2);
        eq("byCategory empty", mm.byCategory(Category.COMBAT).size(), 0);

        int before = a.ticks;
        a.setEnabled(true);
        mm.onTick();
        check("enabled module ticks", a.ticks > before);

        // Search
        ModuleManager sm = new ModuleManager();
        sm.registerAll(new Zoom(), new Cps(), new FpsCounter(), new Coordinates());
        eq("search blank returns all", sm.search("").size(), 4);
        eq("search by name", sm.search("zoom").size(), 1);
        eq("search by category", sm.search("hud").size(), 3);
        check("prefix ranks first", sm.search("c").get(0).name.toLowerCase().startsWith("c"));

        // -------------------------------------------------------------- zoom
        System.out.println("\nZoom");
        Zoom z = new Zoom();
        eq("no zoom when off", z.fovMultiplier(), 1.0);
        check("not zooming when off", !z.isZooming());
        z.setEnabled(true);
        for (int i = 0; i < 400; i++) z.advance();
        check("eases to 1/4 fov", Math.abs(z.fovMultiplier() - 0.25) < 1e-6);
        check("is zooming", z.isZooming());
        check("sensitivity scales down", z.sensitivityMultiplier() < 1.0);
        z.setEnabled(false);
        for (int i = 0; i < 400; i++) z.advance();
        eq("eases back to 1.0", z.fovMultiplier(), 1.0);
        eq("sensitivity restored", z.sensitivityMultiplier(), 1.0);

        // --------------------------------------------------------------- cps
        System.out.println("\nCPS");
        Cps c = new Cps();
        c.setEnabled(true);
        eq("starts at zero", c.leftCps(), 0);
        for (int i = 0; i < 7; i++) c.onLeftClick();
        eq("counts left clicks", c.leftCps(), 7);
        for (int i = 0; i < 3; i++) c.onRightClick();
        eq("counts right clicks", c.rightCps(), 3);
        eq("display both", c.display(), "7 | 3");
        for (int i = 0; i < 2000; i++) c.onLeftClick();
        check("ring is capped", c.leftCps() <= 512);
        c.setEnabled(false);
        eq("disable clears", c.leftCps(), 0);

        // --------------------------------------------------------------- fps
        System.out.println("\nFPS");
        FpsCounter f = new FpsCounter();
        f.setEnabled(true);
        eq("zero before frames", f.fps(), 0);
        for (int i = 0; i < 30; i++) { f.frame(); Thread.sleep(2); }
        check("reports a plausible fps", f.fps() > 0 && f.fps() < 5000);
        check("1% low <= average", f.onePercentLow() <= f.fps() + 1);
        check("display has FPS", f.display().contains("FPS"));
        f.setEnabled(false);
        eq("disable resets", f.fps(), 0);

        // ------------------------------------------------------------ coords
        System.out.println("\nCoordinates");
        Coordinates co = new Coordinates();
        eq("format 0dp", co.format(12.7, 64.0, -33.2), "13, 64, -33");
        eq("overworld to nether", co.convert(800, -400, false), "NE 100, -50");
        eq("nether to overworld", co.convert(100, -50, true), "OW 800, -400");

        // ------------------------------------------------------------ config
        System.out.println("\nConfigIO");
        Path tmp = Files.createTempDirectory("arsex");
        Path cf = tmp.resolve("nested").resolve("modules.json");
        ConfigIO io = new ConfigIO(cf);

        ModuleManager save = new ModuleManager();
        Probe s1 = new Probe("one"), s2 = new Probe("two");
        save.registerAll(s1, s2);
        s1.setEnabled(true);
        s1.setKeybind(72);
        s1.num.set(7.5);
        s1.mode.set("C");
        s1.flag.set(true);
        io.save(save);
        check("file created (parents too)", Files.exists(cf));

        ModuleManager load = new ModuleManager();
        Probe l1 = new Probe("one"), l2 = new Probe("two");
        load.registerAll(l1, l2);
        io.load(load);
        check("enabled restored", l1.isEnabled());
        check("disabled stays off", !l2.isEnabled());
        eq("keybind restored", l1.getKeybind(), 72);
        eq("slider restored", l1.num.get(), 7.5);
        eq("mode restored", l1.mode.get(), "C");
        eq("bool restored", l1.flag.get(), true);
        eq("keybind reindexed after load", load.onKey(72), 1);

        // Settings must be applied before enabling, or onEnable sees defaults.
        eq("onEnable ran exactly once on load", l1.enables, 1);

        // Missing file is not an error.
        ConfigIO missing = new ConfigIO(tmp.resolve("nope.json"));
        missing.load(load);
        check("missing file tolerated", true);

        // Corrupt file degrades instead of throwing.
        Path bad2 = tmp.resolve("bad.json");
        Files.writeString(bad2, "{ \"modules\": { \"one\": { \"enabled\": tru");
        new ConfigIO(bad2).load(load);
        check("truncated file tolerated", true);

        // Escaping round-trip.
        Path esc = tmp.resolve("esc.json");
        ModuleManager em = new ModuleManager();
        Probe ep = new Probe("quote");
        em.register(ep);
        ep.flag.set(true);
        new ConfigIO(esc).save(em);
        String raw = Files.readString(esc);
        check("json has no trailing comma", !raw.contains(",\n  }"));
        check("json is balanced", balanced(raw));

        // ---------------------------------------------------- Stats (launcher feed)
        Path st = Files.createTempDirectory("st");
        Stats stats = new Stats(st.resolve("arsex/stats.json"));
        // Simulate ~60 fps: 16.6ms frames.
        long n = System.nanoTime();
        for (int i = 0; i < 300; i++) { n += 16_600_000L; stats.frame(n); }
        check("stats avg near 60", Math.abs(stats.fpsAvg() - 60) <= 2);
        check("stats max >= avg", stats.fpsMax() >= stats.fpsAvg());
        long now = System.currentTimeMillis();
        stats.maybeFlush(now);
        String sj = Files.readString(st.resolve("arsex/stats.json")).trim();
        check("stats json has avg", sj.contains("\"fpsAvg\":"));
        check("stats json has fresh t", sj.contains("\"t\":" + now));
        check("stats json balanced", balanced(sj));
        // A hitch (alt-tab delta > 1s) must not poison the average.
        n += 5_000_000_000L; stats.frame(n);
        for (int i = 0; i < 130; i++) { n += 16_600_000L; stats.frame(n); }
        check("hitch ignored in avg", Math.abs(stats.fpsAvg() - 60) <= 2);

        // ------------------------------------------------------------- report
        System.out.println("\n" + pass + " passed, " + fail + " failed");
        if (fail > 0) System.exit(1);
    }

    static boolean balanced(String s) {
        int d = 0;
        boolean inStr = false;
        for (int i = 0; i < s.length(); i++) {
            char ch = s.charAt(i);
            if (inStr) {
                if (ch == '\\') i++;
                else if (ch == '"') inStr = false;
                continue;
            }
            if (ch == '"') inStr = true;
            else if (ch == '{') d++;
            else if (ch == '}') d--;
            if (d < 0) return false;
        }
        return d == 0;
    }
}
