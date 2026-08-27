# ARSEX CLIENT — 斬

Monochrome katana Minecraft client. Tauri 2 + Rust launcher, Fabric/Mixin core.

**Status:** verified scaffold. The core compiles and passes 33/33 tests; the
launcher UI runs; the packaging pipeline is written. What is *not* here is the
Mixin injection layer and the 52 module implementations — those need a real
Minecraft dev environment (see [Blockers](#blockers)).

```
arsex-client/
├── prototype/arsex.html          interactive UI — open it, it runs
├── launcher/
│   └── src-tauri/
│       ├── Cargo.toml            pinned deps, size-optimised release profile
│       ├── tauri.conf.json       NSIS bundle, CSP, updater
│       └── src/auth/
│           ├── mod.rs            5-leg Microsoft OAuth + PKCE
│           └── vault.rs          DPAPI token sealing (Win32 FFI)
├── core/
│   ├── src/main/java/dev/arsex/
│   │   ├── module/               Module, Setting, Category, ModuleManager
│   │   └── ui/                   Spring, Ease, Theme
│   ├── src/test/java/Harness.java
│   └── run-tests.sh              ← no Gradle needed, runs in 2s
├── tools/
│   └── mono-lint.mjs             the colour gate
└── docs/ARSEX_SPEC.md            architecture + 52-feature registry
```

## Verify it yourself

```bash
core/run-tests.sh                          # 33 assertions, ~2s
node tools/mono-lint.mjs prototype/        # colour gate
```

Current output:

```
MODULE LIFECYCLE      5/5     enable/disable symmetry, crash containment
SETTINGS              5/5     clamping, step quantisation, mode validation
SEARCH & KEYBINDS     6/6     ranked search, conflict detection
SPRING PHYSICS        9/9     settling, overshoot, framerate independence
EASING CURVES         4/4     bounds, symmetry, clamping
MONOCHROME LOCK       4/4     2048 colour permutations, zero violations
                     ────
                     33 passed, 0 failed
```

## Three bugs the harness caught

Writing tests before the game integration paid for itself immediately.

**1. Static initialiser order (`Theme.java`)** — `INK_000 = lum(0x00)` ran
before `variant = Variant.SUMI` was assigned, because Java initialises static
fields in source order. Every theme constant NPE'd at class-load. Silent in
review, instant crash at runtime. Fixed by hoisting the state fields above the
constant ladder.

**2. Framerate-dependent animation (`Spring.java`)** — the fixed-substep
integrator left an un-consumed accumulator remainder, so the sampled value
drifted by up to one substep depending on frame rate. Measured spread between
60 / 144 / 240 fps was **0.029** — visible as animations landing at subtly
different points on different monitors. Fixed by interpolating across the
remainder and separating `value()` (interpolated, for rendering) from
`rawValue()` (raw simulation state). Spread is now **0.00000**.

**3. `java.lang.Module` collision** — Java 9+ has its own `Module` class, so
`import dev.arsex.module.*` is ambiguous the moment you subclass. Needs an
explicit `import dev.arsex.module.Module;` in every consumer. Worth knowing
before writing 52 of them.

## Background scene

Five hand-authored SVG layers on a parallax rig — no raster art, no generated
images. Total cost is a few KB of markup.

| Depth | Layer | Contents |
|---:|---|---|
| 2 | Sky | Moon with craters, pulsing halo, 10 stars |
| 6 | Far range | Mountain silhouette with snow caps |
| 9 | Birds | Three stroke-pairs crossing on a 38s loop |
| 12 | Near ridge | Second range + five-tier pagoda |
| 16 | Lake | Water plane, 4 breathing ripple lines, moon smear, shoreline |
| 20 | Midground | Torii gate + inverted reflection, two lit stone lanterns with light pools |
| 30 | Foreground | Katana, bamboo grove, turbulence-displaced ink washes |

Plus 3 drifting mist bands and 26 falling ash motes on randomised durations.
The lake is what sells the depth — it gives the scene a floor, doubles the
lantern glow as reflected light pools, and carries a flipped torii that
shimmers out of phase with the ripples.

**Motion.** Pointer position drives `translate3d` per layer, multiplied by
depth, lerped at 0.055 so the scene trails the cursor instead of snapping —
verified as 0.9px travel on the sky vs 13.3px on the blade. A wheel gesture
adds a damped vertical drift that eases back to rest. A single bright glint
travels the blade's `offset-path` every 7s.

**Restraint is the hard part.** The first build was far too loud: the katana
ran diagonally straight through the LAUNCH button and the mountains read as
wireframe diagrams sitting on top of the stat tiles. Fixes:

- Global opacity dropped to 0.62 and the mask fades out by 92% height.
- A radial "hole" is punched behind the launch column so nothing crosses the CTA.
- The blade was moved to the lower-left corner, clear of all controls.
- Ridge strokes cut to 0.13–0.2 opacity so they read as distance, not diagram.

**Accessibility.** Reduced-motion clears every layer transform and short-
circuits the rAF loop rather than merely zeroing durations — verified.

### A dead code path the test caught

The first implementation drove parallax from `p-home`'s `scroll` event. It
never fired: the home page measures 806px in an 806px container, so there is
nothing to scroll. Worse, `.scene` is a *sibling* of `.page` inside `.stage`,
so translating it on page scroll was conceptually wrong regardless. Replaced
with a wheel-driven damped offset that works whether or not content overflows.

## Why the custom cursor was removed

The ink cursor looked good in a still screenshot and was wrong in use:

- It ran a `requestAnimationFrame` loop for the entire session, writing two
  composited transforms every frame whether or not the pointer moved.
- Being a lerped follower, it visibly lagged the real pointer on any dropped
  frame — the one element that must never feel slow.
- `cursor:none` destroyed the OS cursor's signal. A text field stopped looking
  editable and a drag handle stopped looking draggable.

Replaced with real cursors (`pointer`, `text`, `ew-resize`, `grab`/`grabbing`,
`not-allowed`) plus feedback that lives on the element: a `:active` press
scale, and an ink ripple emitted from the click point on `pointerdown` only.

## Keyboard & accessibility

- **Ctrl/Cmd+K** opens the command palette.
- **Alt+1…7** jumps straight to a page; **`[`** and **`]`** cycle.
- Every `[data-hot]` element gets `tabindex`, `role="button"` and Enter/Space
  activation, so nothing is mouse-only.
- Focus rings use `:focus-visible`, so they appear for keyboard users and stay
  out of the way of mouse users.

## Command palette

One fuzzy field over three sources: **actions** (create instance, launch,
toggle Click GUI, cloud sync, reduced motion), **pages**, and all **63
modules** — selecting a module navigates to it, scrolls it into view and
toggles it. Arrow keys move, Enter runs, Escape closes, a footer shows the
bindings and a live result count.

Ordering was fixed after a render pass: pages were listed first, so eight
"Go to" rows filled the entire first screenful and pushed every real command
below the fold. Actions now rank first, then pages, then modules.

## Performance pass

| Change | Effect |
|---|---|
| Cursor rAF loop deleted | One permanent animation loop and 2 composited layers gone |
| Wheel `setInterval(1000/60)` folded into the parallax rAF | Two uncoordinated loops became one; no more mid-frame transform writes |
| Idle short-circuit in the scene loop | Once settled, zero transform writes — a static screen costs ~0 style work |
| `visibilitychange` gate | Scene animation stops entirely on a hidden tab |
| Cached `getBoundingClientRect` | Removed a forced layout on every `mousemove` |
| `mousemove`/`wheel` marked `passive` | No scroll/input blocking |
| HUD keystroke demo gated on page visibility | Was firing 4×/s for the whole session on a page usually hidden |
| Reduced-motion parks once via a flag | Was clearing every layer's transform on every frame |

## The launch engine (`launcher/core-launch`)

A standalone crate with **no Tauri dependency**, so it builds and tests on any
host including CI without a desktop stack. This is what makes the client
actually launch Minecraft rather than animate a progress bar.

| Module | Responsibility |
|---|---|
| `manifest.rs` | Version JSON, Mojang rule engine, `inheritsFrom` merging |
| `install.rs` | Download planning, SHA-1 verification, natives extraction, classpath |
| `args.rs` | Placeholder substitution, argv construction, token redaction |
| `mods.rs` | Real jar metadata, dependency + conflict validation |

### Verified against the live Mojang API

Six integration tests run against production endpoints
(`cargo test --test live_mojang -- --ignored`):

```
manifest: 908 versions, latest release 26.2
1.8.9    java 8  · 37 libs (34 apply) · 32 cp · net.minecraft.client.main.Main
1.12.2   java 8  · 39 libs (34 apply) · 33 cp
1.16.5   java 8  · 57 libs (41 apply) · 34 cp
1.20.4   java 17 · 88 libs (64 apply) · 51 cp
26.2     java 25 · 131 libs (88 apply) · 66 cp
1.20.4 libs — win 64 · linux 52 · osx 58
assets: 3811 objects, 0.65 GB
downloaded 964 bytes, sha1 verified
```

That spans every format era Mojang has shipped: the pre-1.13
`minecraftArguments` string, the modern `arguments` block, Java 8 through 25.

### Details that decide whether the game starts

- **Rule evaluation order.** Rules apply in sequence, last match wins, and the
  default with a rule list present is *disallow*. Get this wrong and you either
  ship Linux natives to Windows or exclude every library.
- **Classpath order is load-bearing.** Modloader libraries must precede vanilla
  so patched classes shadow originals, and the client jar must be **last**.
  Duplicates dedupe on `group:artifact` keeping the *first*, so a loader's
  pinned version wins.
- **Natives-only libraries are excluded from the classpath** but still extracted.
- **`extract.exclude` is honoured** — leaving `META-INF/` signatures in place
  makes the JVM reject the extracted natives.
- **SHA-1 verification everywhere**, with atomic temp-file-then-rename writes so
  an interrupted download can't leave a corrupt file that passes an existence
  check next run.
- **Zip-slip is blocked** in natives extraction.

### Token safety

The access token must appear in argv (Mojang's protocol requires it), but it
must never reach a log. `args::redact()` scrubs `--accessToken`, `--session`
and any embedded occurrence; the console tab and every on-disk log run through
it. Placeholder substitution is **single-pass and non-recursive**, so a player
named `${auth_access_token}` cannot expand into the real token — there is a
test for exactly that.

## Real mod integration

The old My Mods tab derived a mod's name and loader from its *filename*.
`sodium-fabric-mc1.20.1-0.5.3.jar` is not a mod called "Sodium Fabric Mc".

The engine now opens the jar and reads what the mod declares:

| Loader | Metadata |
|---|---|
| Fabric | `fabric.mod.json` |
| Quilt | `quilt.mod.json` (nested under `quilt_loader`) |
| Forge 1.13+ | `META-INF/mods.toml` |
| NeoForge | `META-INF/neoforge.mods.toml` |
| Forge legacy | `mcmod.info` |

Pre-launch validation catches the failures that actually break a modpack:
missing hard dependencies, duplicate mod ids, and loader mismatches. Problems
surface as `launch://mod-problem` events in the console *before* the JVM
starts, instead of as a stack trace after it dies.

Enable/disable renames to `.jar.disabled`, the convention other launchers use,
so mod folders stay portable.

**A parser bug caught by the tests:** discriminating Forge TOML from legacy
JSON on `starts_with('[')` is wrong, because `[[mods]]` also starts with `[`.
Real `mods.toml` files were being routed to the JSON parser.

## Demo mode — community pre-release testing

Testers need to exercise the **launcher**; that does not require owning
Minecraft. Demo mode unlocks every screen with a synthetic local profile so the
community can file bugs against the UI, console, wizard and mod manager.

| Works in demo | Blocked in demo |
|---|---|
| Every page, console, wizard, mod manager | Launching Minecraft |
| Settings, HUD editor, profiles persist | Joining any server |
| Full keyboard nav and command palette | Obtaining any session |

### What it is not

This is **not** a cracked/offline-account path, and the difference is
structural rather than a policy toggle:

- `DemoProfile` **has no token field**, so there is nothing to forge a session
  with. A unit test serialises it and asserts no credential appears.
- `demo::can_launch()` is a `const fn` returning `false`, checked in
  `main.rs::launch_game` — the single chokepoint every launch path funnels
  through. No argv route reaches the JVM.
- Mojang is never contacted. No fake session server, no auth interception, no
  `--uuid`/`--accessToken` forgery. It cannot join an online server because it
  never obtains a session **at all**.
- Demo state is memory-only and dies with the process, so it can never
  masquerade as a real login later.
- Demo UUIDs force the version nibble to `8`, so they can never be mistaken for
  a real Mojang v4 UUID by any downstream parser.

Defence in depth on the launch gate: CSS disables the button, the JS handler
refuses and redirects to sign-in, and Rust refuses the spawn. The Puppeteer
suite force-clicks past the CSS to prove the inner guards hold.

### Why not a cracked launcher

It would make the client unshippable: it cannot be code-signed with the EV
certificate the build script expects, Hypixel and every major server ban
clients that support offline auth, and it destroys the exclusive identity that
is the entire positioning. Legitimate testing needs are met above.

## Game console

Pressing LAUNCH attaches a console tab and switches to it. The tab is hidden
until a session exists, and its badge pulses while the JVM is alive.

- **Structured lines** — timestamp, thread, level, message. WARN gets a left
  rule and lifts to full white; ERROR gets a brighter rule and steel text;
  DEBUG recedes to grey.
- **Level filters + text search**, with matches highlighted inline.
- **Follow-tail** that disengages on user intent and a FOLLOW TAIL button to
  re-engage.
- **Footer telemetry** — PID, line count, warn/error counts, uptime, exit code.
- **COPY / SAVE LOG / CLEAR / END TASK.** SAVE LOG writes a real file.
- **2000-line ring buffer**; rows are appended incrementally, and a full
  repaint happens only when a filter changes.

### The follow-tail race

Position-based auto-follow is subtly broken: a log line arriving between the
user's scroll and the scroll event re-pins the view to the bottom, so the user
can never scroll up to read. The fix is to disengage on **intent** (wheel-up,
pointer-down, PageUp/Home) and only *re-engage* from position, with
programmatic scrolls flagged so they are never mistaken for user input.

### Where the lines come from

| Build | Source |
|---|---|
| Browser prototype | Scripted emitter — a realistic boot + steady-state sequence |
| Packaged `.exe` | Real JVM stdout/stderr via `game://log` events from Rust |

`tools/sync-frontend.mjs` injects the native bridge when building, so the two
can never drift.

## Building the real .exe

**See [`docs/BUILDING.md`](docs/BUILDING.md) for the full guide.** Short version:

**No Windows machine?** Push to GitHub. `.github/workflows/build.yml` builds on
a `windows-latest` runner and uploads `arsex.exe` + installer as artifacts;
pushing a `v*` tag publishes a public release for testers.

**On Windows?** `pwsh tools\build.ps1`.

```powershell
$env:ARSEX_AZURE_CLIENT_ID = "<your-azure-app-id>"
pwsh tools\build.ps1              # add -Sign with ARSEX_CERT_THUMBPRINT
```

Produces a standalone `arsex.exe` plus an NSIS installer. The script gates on
the Azure client ID (compiled in via `env!()`, so the build cannot proceed
without it), the monochrome lint, the Rust tests and the Java harness before it
will compile, then reports artifact sizes against the 12 MB budget.

**Verified on this machine:** `cargo check` and `cargo test` both pass
(5/5 Rust tests). `cargo check --target x86_64-pc-windows-msvc` gets as far as
`ring`, which needs `link.exe` — cross-linking MSVC from Linux is not
supported, so the final bundle step must run on Windows.

### Native pieces

| File | Role |
|---|---|
| `src/main.rs` | Entry point, `windows_subsystem = "windows"` (no console flash), single-instance, panic→crash-report hook, IPC commands |
| `src/paths.rs` | Roaming vs local split; instance slugs validated against path traversal |
| `src/game/process.rs` | Spawns the JVM with `CREATE_NO_WINDOW`, pumps stdout+stderr on threads, parses Log4j lines, mirrors to disk, reaps and reports exit code |
| `src/auth/mod.rs` | OAuth chain + the command layer. **No token ever crosses into the webview** — only a display profile |
| `icons/` | Hand-drawn monochrome mark, full `.ico` ladder (16→256), measured channel spread 0 |

## New Instance wizard

Clicking **NEW INSTANCE** opens a four-step modal rather than firing a toast.

| Step | Contents |
|---|---|
| Identity | Name field with live validation, 6 icon tiles |
| Version | 5 MC versions, 4 mod loaders |
| Resources | Memory slider (1–16 GB, diamond knob), 3 option toggles |
| Review | Full summary table, then CREATE |

Details that matter:

- **Validation is live.** Empty blocks CONTINUE; invalid characters and
  duplicate names each get their own message; a valid name previews the folder
  slug (`instances/ranked-duels`).
- **Recommended memory adapts to the version** — 4 GB for 1.8.9, 6 GB for
  1.20.4/1.21.4.
- **Panes slide directionally** — forward enters from the right, BACK enters
  from the left, so the motion encodes direction.
- **Step rail underlines wipe** from centre-out on the active step; completed
  steps hold a dim full-width rule.
- **Creation replays the boot animation** — the 複 mark pulses while a blade of
  light is drawn across with a glowing tip, through six real stages
  (game root → manifest → libraries → assets → loader → finalise).
- **Dismissal**: X button, Escape, or backdrop click. Enter advances.

Verified with 18 Puppeteer assertions covering every path including
validation rejection, back-navigation, and all three dismissal routes.

Two fixes from the render pass: the creation overlay was translucent so the
summary table bled through it (now opaque, with the layer beneath blurred),
and the close button sat on top of the 複 watermark.

## My Mods — custom mod installation

A real working tab, not a mock. `prototype/arsex.html` → **My Mods**.

| Capability | Behaviour |
|---|---|
| Install | Drag-and-drop onto the zone, or click to browse. Accepts multiple files. |
| Validation | Non-`.jar`/`.zip` files are rejected with a count, not silently dropped. |
| Metadata | Parses filename for name, version and loader (Fabric/Forge detected). |
| Compatibility | Version token compared against the selected MC build; mismatches get an `UNTESTED` tag and a warning toast. |
| Duplicates | Detected by filename and flagged, but still allowed. |
| Toggle | Same visual language as core modules — custom mods aren't second-class. |
| Remove | Row collapses and slides out before the array mutates. |
| Persistence | `localStorage`, survives reload. |

Verified end-to-end with Puppeteer driving real `File` objects — **11/11
assertions pass**, including a genuine upload through the file input and a
full page reload to confirm persistence.

Three fixes the render pass forced:

1. **Titles duplicated the version.** `iris-1.6.14-1.20.1.jar` became
   "Iris 1.6.14 1.20.1" *and* carried a `1.6.14` tag. Version tokens and loader
   words are now stripped from the display name.
2. **Rows spanned the full 1440px**, leaving a metre of dead space beside 40px
   of content. Capped at 920px.
3. **Toast stack filled the viewport** during rapid installs. Capped at 3.

## Layout: horizontal top nav (Lunar-style)

The vertical sidebar was replaced with a horizontal top bar, matching the
reference. Structure is now:

```
┌──────────────────────────────────────────────────────┐
│ titlebar                                             │
├──────────────────────────────────────────────────────┤
│ Home  Modules  HUD │ Cosmetics  Packs  Settings   [account] │
├──────────────────────────────────────────────────────┤
│      ╱figure╲      斬                  ╱figure╲       │
│                [ version chips ]                     │
│                [    LAUNCH     ]                     │
│                 profile · memory                     │
├──────────────────────────────────────────────────────┤
│  AVG FPS  │  MEMORY  │  PLAYTIME  │  FRAME TIME      │
├───────────────────────────────────┬──────────────────┤
│  LATEST NEWS  (3 cards)           │  FRIENDS         │
└───────────────────────────────────┴──────────────────┘
```

Notes on the translation:

- **Active-state indicator rotated with the layout.** The sidebar used a 3px
  left rule; horizontally that becomes a bottom underline animating from
  `left:50%/right:50%` outward, so it wipes open from the centre.
- **Hero is a single centred launch column** — mark, version chips, LAUNCH,
  profile meta. Character art was trialled and pulled; it crowded the focal
  point and the button reads stronger alone.
- **Nav is grouped**, not a flat list: primary (Home/Modules/HUD), a hairline
  separator, then library (Cosmetics/Packs/Settings). Accounts moved into the
  account chip on the right, where users expect it.
- **News art is generated SVG** — ink-wash compositions built from the same
  brush-stroke vocabulary as the boot screen.

### Two bugs the render caught

**1. A CSS block got eaten.** My HOME section rewrite overwrote the region that
defined `.chip`, `.stats`, `.stat` and `.spark`. An inline SVG with only a
`viewBox` and no height expands to fill its container, so the sparkline grew to
~500px and consumed the entire page below the hero. Restored with an explicit
`height:24px; display:block`.

**2. The background katana sliced through the news cards.** It was positioned
for a sidebar layout; once content went full-width the diagonal cut across the
cards and read as a rendering artefact. Now clipped to a 400px hero band with a
gradient mask, at 0.3 opacity.

Verified after the change: all 7 pages navigate, zero horizontal overflow, zero
console errors, still zero rounded corners.

## Corner language: chamfer, not radius

Rounded corners were cut entirely — `--r-*` tokens are all `0px` and a
Puppeteer sweep of every DOM node confirms **zero** elements with a non-zero
`border-radius`. A blade has no soft edges, and the radii were quietly
contradicting the whole concept.

Sharp alone is just plain, though, so the definition moved elsewhere:

- **45-degree chamfers** on buttons, cards, panels, windows and toasts. A cut
  corner reads as *forged/machined*; a radius reads as *moulded*.
- **Registration ticks** (`.framed`) — L-marks at the top corners of large
  panels, borrowed from print crop marks and optical sight reticles.
- **Diamond slider knobs** and a **diamond cursor** that spins 45° on click.
- **Blade edge** on active cards: a full-height 2px white rule with a soft
  bloom, scaling up from `scaleY(0)`.

### Three flaws the screenshot pass caught

Rendering at 2× and actually looking at it beat reading the CSS:

**1. Chamfers had open corners.** `clip-path` removes the border along the
diagonal, so every cut corner was a gap in the hairline — the whole UI looked
subtly unfinished. Fixed with a `.ch` utility that redraws the diagonal as a
1px background gradient sized to the chamfer box.

**2. Registration ticks were applied to every `.glass`.** With 60 module cards
on screen that meant ~240 bright white marks competing with the content. Made
them opt-in via `.framed`, so only large panels get them.

**3. Solid-white toggles overpowered everything.** Thirty filled rectangles
were louder than the module names they belonged to. The card's blade edge is
already the primary "on" signal, so the toggle now confirms state with a
glowing knob on a dim slot instead of shouting it.

## Motion pass

Every transition was re-fitted to a three-curve family derived from the Java
core's spring constants, so launcher and in-game motion match:

```
--e-spring  cubic-bezier(.22,1.4,.36,1)   ≈ snappy    k380 c26
--e-soft    cubic-bezier(.33,1,.28,1)     ≈ smooth    k210 c30
--e-heavy   cubic-bezier(.4,.9,.2,1)      ≈ cinematic k90  c19
```

What changed:

- **Page transitions** now blur-and-rise (`blur(6px)` → 0) instead of a flat
  fade, and children cascade in at 40ms intervals — a page change reads as a
  composed sequence, not one block appearing.
- **Cards** lift 5px *and* scale 1.012 on hover, with an inset top highlight;
  they compress to 0.995 on press with a 90ms transition so the click feels
  mechanical.
- **Click GUI windows** drop in from -16px with blur and scale, staggered 45ms
  apart.
- **Toasts** arrive rotated 1.5° with blur, like they were thrown.
- **LAUNCH** has a 4.2s breathing glow so the focal point is never inert; the
  animation cancels on hover so it doesn't fight the interaction.
- **Mod rows** slide in from -22px and collapse with negative margin on delete,
  so the list closes the gap rather than snapping.

Reduced-motion still overrides all of it — verified post-change.

## Why springs, not easing curves

The Click GUI is the thing users touch most, and the tell of a cheap client is
animation that *restarts* when interrupted. A duration-based curve retargeted
mid-flight snaps back to t=0 and produces a visible hitch. A spring carries its
velocity across the retarget, so motion stays continuous no matter how fast you
spam a toggle.

The harness asserts this directly: `velocity carries across retarget` checks
that v=9.1 survives a target flip from 1 → 0.

Three presets, matched 1:1 to the launcher's CSS tokens so in-game and
out-of-game motion are indistinguishable:

| Preset | k | c | Behaviour | Used by |
|---|---|---|---|---|
| `snappy` | 380 | 26 | ~5% overshoot, 639ms | toggles, knobs |
| `smooth` | 210 | 30 | zero overshoot | page transitions |
| `cinematic` | 90 | 19 | heavy, deliberate | boot, launch |

Integration is a fixed 1/240s substep with a 0.1s frame clamp, so an alt-tab
stall can't fling a panel across the screen.

## The monochrome lock

Not a guideline — a build gate. `mono-lint.mjs` parses every colour literal in
compiled output (hex 3/4/6/8-digit, `rgb()`, `rgba()`, `hsl()`, `hsla()`, and
147 named CSS colours) and exits non-zero on any non-zero saturation.

On the Java side, `Theme.lum(int)` is the only colour constructor in the
codebase. It takes a single luminance byte. There is no parameter through which
a hue could be expressed.

Verified against a deliberately poisoned file:

```
#3b82f6            → rgb(59,130,246) spread 187     FAIL
rgba(255,87,34,.8) → rgb(255,87,34)  spread 221     FAIL
hsl(210,90%,55%)   → saturation 90%                 FAIL
crimson            → named colour                   FAIL
#1a1a1a            → spread 0                       pass
```

## Authentication

Five legs, no shortcuts:

```
MSA (PKCE S256) → Xbox Live → XSTS → Minecraft Services → Profile
```

Decisions worth defending:

- **System browser, not embedded WebView.** An in-app login form is a
  credential-phishing pattern and Microsoft discourages it. Arsex never sees a
  password and structurally cannot.
- **Loopback on `127.0.0.1:0`.** OS-assigned ephemeral port. Hardcoding one
  collides with other launchers and with a second Arsex instance.
- **DPAPI over Credential Manager.** WCM entries are enumerable by any process
  running as the user and are bulk-readable by common dumping tools. DPAPI
  with per-install entropy is a smaller target, and we control the file.
- **XSTS errors are translated.** `2148916233` becomes *"This account has no
  Xbox profile — sign in at xbox.com once."* Most launchers surface the raw
  number, which tells the user nothing.
- **Entitlement check before profile fetch.** Catches Game Pass expiry and
  non-owning accounts with a clear message instead of a 404.

## Blockers

Four things must come from you before an `.exe` exists:

| Blocker | Detail |
|---|---|
| **Azure client ID** | Register at portal.azure.com, then apply for Minecraft API access. Until approved, leg 4 returns 403. The code reads it via `env!("ARSEX_AZURE_CLIENT_ID")` at compile time. |
| **EV certificate** | ~$350/yr. An OV cert does **not** grant instant SmartScreen trust — reputation accrues over weeks of download volume. Since June 2023 keys must live on FIPS 140-2 L2 hardware, so CI signing needs a cloud HSM (Azure Trusted Signing, DigiCert KeyLocker), not a checked-in `.pfx`. |
| **No redistribution** | You may not ship Minecraft or its assets. The launcher must download official artifacts from Mojang's CDN under the user's own entitlement. |
| **EULA / anticheat** | All 52 features in the spec are deliberately server-legal QoL. No killaura, no reach extension, no flight. Ship combat automation and Hypixel bans the client in a week. |

## Build

```powershell
$env:ARSEX_AZURE_CLIENT_ID = "<your-app-id>"
$env:ARSEX_CERT_THUMBPRINT = "<cert-sha1>"
pwsh build\build.ps1 -Sign
```

Gates in order: mono-lint → core tests → rust tests → jar → UI bundle →
`cargo tauri build` → signtool (inner exe *and* installer) → update manifest.

## Budgets

| Metric | Target |
|---|---|
| Installer | < 12 MB |
| Cold start → interactive | < 1.2 s |
| Idle RAM | < 60 MB |
| UI frame time @144Hz | < 4 ms |
| Toggle → visible | < 16 ms |
| In-game overhead | < 2% frametime |

Tauri over Electron is what makes these reachable: WebView2 is already on every
Win10 20H2+ machine, so there is no 150 MB Chromium copy and no ~180 MB idle
baseline. Shipping a browser alongside the game would contradict the entire
performance pitch.

---

*斬 — one cut, clean.*
