# Arsex Mod

The in-game half of Arsex Client: a Fabric mod that adds working modules, a
configuration menu, and a monochrome HUD to Minecraft itself.

The launcher starts the game. This runs *inside* it.

---

## What it does

| Module | Category | Default key | What it actually does |
|---|---|---|---|
| Fullbright | Visual | `B` | Drives the vanilla gamma option past its slider cap. Works with shaders, underwater and in the Nether, because it scales a value the game already respects everywhere. Captures and restores the original gamma, with optional easing. |
| Zoom | Visual | `C` | Scales the *computed* FOV via a mixin on `GameRenderer#getFov`, so it composes correctly with sprint FOV and speed effects. Eased, with matching mouse-sensitivity scaling. |
| CPS Counter | HUD | — | Sliding one-second window over click timestamps. Left and right tracked separately. Hard-capped at 512 entries so a drag-click cannot grow it without bound. |
| FPS Counter | HUD | — | Rolling 240-frame ring with a **1% low** figure — the number that actually correlates with perceived smoothness, which vanilla's integer average hides. |
| Coordinates | HUD | — | Position readout with Nether/Overworld conversion and compass facing with axis signs. |

Every module has settings, every setting is exposed in the in-game menu, and
everything persists.

## The configuration menu

Press **Right Shift** in game.

- **Left click** a module — toggle it
- **Right click** — expand its settings
- **Middle click** — rebind it (then press a key; `Esc` clears the bind)
- **Esc** — close and save

Columns are one per category. Sliders step on click; modes cycle; booleans
toggle. Values are clamped and snapped in `Setting`, so the GUI physically
cannot produce an out-of-range value.

The menu does **not** pause singleplayer (`shouldPause()` returns false), so
you can tune a setting and watch it take effect live.

## Config

Written to `.minecraft/config/arsex/modules.json`:

```json
{
  "version": 1,
  "modules": {
    "fullbright": {
      "enabled": true,
      "keybind": 66,
      "settings": { "Level": "10.0", "Fade": "true" }
    }
  }
}
```

Flat `string -> string` by design, so the launcher can read and write it
without the two halves having to agree on a JSON library. Saves go through a
temp file and an atomic rename — a crash mid-save cannot truncate your config.
A corrupt or hand-edited file degrades to defaults instead of throwing.

Settings are applied **before** the module is enabled on load, so `onEnable()`
never sees stale defaults.

## Building — real, against real Minecraft

```bash
cd mod
./gradlew build          # -> build/libs/arsex-mod-<version>.jar
```

Requirements: JDK 17 (Minecraft 1.20.4's toolchain) and network access to
maven.fabricmc.net + piston-meta (first build only — loom caches Minecraft
and the Yarn mappings). The Gradle wrapper (8.10.2) is committed; there is
nothing to install besides the JDK.

CI does exactly this in the `mod` job and attaches the jar to releases, so
you normally never build it by hand.

### What the first real compile caught (fixed)

These classes are never touched by the offline harness, and the first actual
compile-plus-remap against 1.20.4 Yarn `1.20.4+build.3` proved two of them
broken:

- `InGameHudMixin` declared a `RenderTickCounter` parameter — a **1.20.5+**
  signature. 1.20.4's `InGameHud#render` takes `float tickDelta`
  (`method_1753(class_332;F)V` in the shipped refmap). Mixin would have
  refused the handler at apply time and, with `defaultRequire: 1`, **crashed
  the game on startup**. Now fixed to the real signature.
- `Fullbright` called `options.getGamma().setValue(10.0)`. Verified in the
  1.20.4 bytecode: `SimpleOption.setValue` routes through
  `DoubleSliderCallbacks.validate`, which returns `Optional.empty()` outside
  [0.0, 1.0] — so the call was *silently ignored* and fullbright did nothing.
  The module now writes the backing value through a `SimpleOption` accessor
  mixin (`SimpleOptionAccessor`) when above the cap and restores through the
  front door on disable.
- `GameRenderer#getFov(Camera, float, boolean)` and
  `Mouse#onMouseButton(long, int, int, int)` and `options.hudHidden` were all
  confirmed correct against the actual jar — no changes needed.

### Not verified here

The jar compiles, remaps and its refmap matches 1.20.4. Nobody in this repo's
toolchain has *launched the game with it* and pressed Right Shift. CI builds
the jar; it does not boot Minecraft. First in-game session is the remaining
gate.

## Testing

Two independent gates:

```bash
bash mod/run-tests.sh    # 71 assertions, ~2s, no Gradle and no network
cd mod && ./gradlew build  # the real compile against real Minecraft
```

The first covers the module system, settings, config round-trip and the four
pure-logic modules without a Minecraft instance. This is deliberate: anything
that can be tested without the game *is* kept free of Minecraft types — which
is why `Zoom` owns easing maths but no rendering, and the mixins are thin
adapters holding no logic of their own. The second gate is what type-checks
the GUI, the HUD, Fullbright and the mixins.

## Structure

```
module/     Module, Setting, Category, ModuleManager  — no Minecraft types
modules/    the five concrete modules
mixin/      GameRenderer (zoom), Mouse (cps), InGameHud (hud) — thin adapters
gui/        ClickGui — the in-game configuration menu
hud/        HudRenderer — monochrome overlay
config/     ConfigIO — atomic, dependency-free JSON
```

### Two invariants worth keeping

1. **`onDisable()` must fully revert `onEnable()`.** That is what makes
   hot-toggling safe and lets config restore state without a restart.
2. **Greyscale only.** Every ARGB constant has equal R, G and B channels. The
   client's identity is the absence of colour; the HUD failure state signals
   through a halted mark and a stalled bar, never a red.

A module that throws is caught, logged and disabled rather than taking down
the tick loop or the game.
