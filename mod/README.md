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

## Building — NOT YET POSSIBLE

> **Status: this mod has never been compiled against Minecraft and there is no
> downloadable jar.** Do not assume any of the in-game behaviour described
> above actually works yet. The parts that are genuinely verified are listed
> under Testing.

`build.gradle` and `gradle.properties` exist, but the project cannot be built
because these are missing:

- `settings.gradle`
- the Gradle wrapper (`gradlew`, `gradlew.bat`, `gradle/wrapper/`)
- `src/main/resources/assets/arsex/icon.png`, referenced by `fabric.mod.json`

Once those exist, the intended flow is JDK 17 (Minecraft 1.20.4's toolchain):

```bash
cd mod
./gradlew build          # -> build/libs/arsex-mod-2.4.1.jar
```

...then drop the jar in `mods/` alongside Fabric API.

### What the first real compile will probably break

These classes are **never** touched by the offline harness, so nothing has
type-checked them:

```
ArsexMod.java   gui/ClickGui.java   hud/HudRenderer.java
modules/Fullbright.java             mixin/*.java
```

The mixin targets were written against 1.20.4 Yarn names from memory. Expect
to correct at least:

- `InGameHudMixin` takes `RenderTickCounter`, which is a **1.20.5+** signature.
  On 1.20.4 `InGameHud#render` takes a `float tickDelta`.
- `Fullbright` assumes `SimpleOption` accepts a gamma above `1.0`.
- `mc.options.hudHidden` — field name unverified on this version.

## Testing

What **is** verified: the module system, settings, config round-trip and the
four pure-logic modules, tested without a Minecraft instance at all. This is a
real gate, but note what it does *not* cover — the GUI, the HUD, Fullbright and
all three mixins are excluded, because they import Minecraft types:

```bash
bash mod/run-tests.sh    # 71 assertions, ~2s, no Gradle and no network
```

This is deliberate. Anything that can be tested without the game *is* kept
free of Minecraft types — which is why `Zoom` owns easing maths but no
rendering, and the mixins are thin adapters holding no logic of their own.

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
