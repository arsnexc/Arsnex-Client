# Arsex Client — handoff to the next session

Paste the section marked **PROMPT** into a fresh session. Everything below it
is context that session will need.

---

## PROMPT

> I'm continuing work on **Arsex Client**, a monochrome Japanese-katana-themed
> Minecraft client for Windows 10/11. The workspace is in this conversation's
> files; the source of truth is <https://github.com/arsnexc/Arsnex-Client> at
> commit `ea11482`, tag `v2.4.1`.
>
> GitHub token (my alt account, use it to push):
> `REPLACE_WITH_YOUR_TOKEN`
>
> Read `HANDOFF.md` in the repo root first — it has the full state, the
> constraints I've set, and the traps that already cost previous sessions time.
>
> **The one thing I need fixed:** the Fabric mod does not actually run in game.
> There is no downloadable `.jar`, so no modules and no in-game configuration
> menu exist for a real player. Java source was written and unit-tested, but it
> has never been compiled against Minecraft and never shipped.
>
> Do this, in order:
>
> 1. Make `mod/` actually buildable — it is missing `settings.gradle`, the
>    Gradle wrapper, and the mod icon. Verify with a real `./gradlew build`
>    that produces `arsex-mod-2.4.1.jar`, not just the offline harness.
> 2. Fix whatever the real compile reveals. The mixins were written against
>    1.20.4 Yarn signatures from memory and have **never** been compiled — expect
>    `GameRenderer#getFov`, `InGameHud#render` and `Mouse#onMouseButton` to need
>    correcting against the actual mappings.
> 3. Add a CI job that builds the jar and attaches it to the release, so a tag
>    produces `arsex.exe` **and** `arsex-mod-2.4.1.jar`.
> 4. Make the launcher install the mod into an instance automatically, so a
>    player who clicks LAUNCH gets the modules without hand-copying a jar.
> 5. Tag `v2.5.0` and give me direct release download links, verified with an
>    anonymous request so I know they work without logging in.
>
> Do not claim anything works until a real build proves it. If you cannot
> verify something, say so plainly.

---

## Where things actually stand

### Shipped and working

- **Launcher `.exe`** — real, downloadable, CI-built.
  <https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.4.1>
  - `arsex.exe` 5.79 MB · `Arsex.Client_2.4.1_x64-setup.exe` 2.18 MB
  - Both verified anonymously (HTTP 200, correct `content-length`).
- **New Instance wizard is genuinely real.** It calls `create_instance` in
  Rust: real directories, real Mojang manifest fetch, SHA-1 verified library
  and asset downloads, real `instance://stage` progress events. It shares the
  launch cache, so first launch re-downloads nothing. This replaced a
  `setInterval` that faked six progress steps.
- **Launch pipeline** (`launcher/core-launch`, crate `arsex-launch`) — 55 tests,
  tauri-free. Classpath dedupe, natives extraction, SHA-1 verification.
- **Official free demo, implemented (v2.5.1).** A real MSA sign-in on an
  account without Java entitlement no longer dead-ends: the account is saved
  as `owns_game: false` (serde-defaults true for older vaults), LAUNCH
  resolves a genuine session IN RUST (`auth::resolve_launch_identity` ->
  `login_silent`; the token never crosses into the webview), and the launch
  context sets `demo`, which the real 1.20.4 JSON expands to Mojang's own
  `--demo` argument — proven by a live piston-meta test. The UI-demo path
  (`begin_demo`) is unchanged and still cannot launch.
- **The Fabric mod now builds for real** (v2.5.0 work):
  - `mod/settings.gradle` + committed Gradle wrapper (8.10.2) +
    `assets/arsex/icon.png` (hand-authored, greyscale-asserted,
    `tools/make-mod-icon.py`) + `mod/LICENSE`.
  - `./gradlew build` compiles against real Minecraft 1.20.4 + Yarn
    `1.20.4+build.3` under fabric-loom 1.7.4 and produces
    `build/libs/arsex-mod-2.5.0.jar` (~45 KB) with a correct refmap.
  - **Two real bugs the first compile + a live `runClient` exposed and fixed:**
    1. `InGameHudMixin` used the 1.20.5+ `RenderTickCounter` signature; 1.20.4
       is `render(DrawContext, float)` (`method_1753(class_332;F)V`). With
       `defaultRequire: 1` the old code would have crashed the game on startup.
    2. `Fullbright`'s `getGamma().setValue(10.0)` was **silently ignored** —
       1.20.4 `SimpleOption.setValue` routes through
       `DoubleSliderCallbacks.validate`, which empties the Optional outside
       [0,1]. Fixed with a `SimpleOptionAccessor` mixin that writes the
       backing field above the cap.
  - Verified by actually booting the game headless (Xvfb + llvmpipe,
    `./gradlew runClient`): fabric-loader 0.15.11 loaded, all four mixins
    applied, `(Arsex) Arsex client initialised with 5 modules` in the live
    log, `arsex` in the ResourceManager pack list. First frame never finished
    under llvmpipe on 2 cores, so **menu interaction / gameplay remain
    unverified** — that stays the final gate.
  - CI `mod` job builds the jar; the Windows build embeds it via
    `launcher/src-tauri/resources/arsex-mod.jar` + `include_bytes!` (build.rs
    sets `arsex_mod_bundled`); releases attach exe + installer + jar.
- **Launcher auto-installs the Fabric stack.** Pressing LAUNCH on a FABRIC
  instance now provisions, for real: the loader profile from
  meta.fabricmc.net (`core-launch/src/fabric.rs`, pinned loader 0.15.11),
  fabric-api 0.97.0+1.20.4 (pinned SHA-256), and the embedded Arsex jar —
  all idempotent, and a jar the user disabled (`.jar.disabled`) stays
  disabled. Before this, "FABRIC" was a label that resolved to vanilla.
- **CI** — `.github/workflows/build.yml`, four jobs: `verify`, `mod`,
  `build` (Windows x64, needs mod), `release` (needs build+mod, gated on
  `refs/tags/v*`).

### v2.5.0 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.5.0> — all assets
verified anonymously (HTTP 200, correct content-length):

- `arsex.exe` 5.85 MB (jar embedded)
- `Arsex.Client_2.5.0_x64-setup.exe` 2.23 MB
- `arsex-mod-2.5.0.jar` 45,583 bytes — sha256
  `aed878a6028b69448b2666b738cf163aae144e6c95d252743870f5933eb09baa`,
  byte-identical to the locally verified build.

### v2.5.1 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.5.1> — all assets
verified anonymously (HTTP 200, correct content-length): `arsex.exe` 5.86 MB
· `Arsex.Client_2.5.1_x64-setup.exe` 2.23 MB · `arsex-mod-2.5.0.jar`
(byte-identical to v2.5.0's, mod unchanged). This is the official-demo
release; CI needed two rounds (a missing Ok/Err match arm in main.rs that
only the Windows compile catches — the local gate cannot compile main.rs).

### v2.5.2 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.5.2> — fixes the
'mod does not run in game' trap: LAUNCH / My Mods / installs all hardcoded
instance 'main', so a wizard instance under any other name launched an
empty vanilla copy. An INST module in the prototype is now the single
source of truth (hero chips are instance selectors; real slug, version and
memory on launch; wizard selects what it creates; no-instance -> wizard,
never a phantom launch). **tools/insttest.mjs is committed** — 16 headless
Chrome assertions covering all of it; the old lost Puppeteer suites stay
gone, this one cannot be.

### v2.5.3 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.5.3> — motion/UX
pass: REAL launch progress on the hero button (bar + % + live stage label,
driven by actual launch://stage events; the fake two-second sequence is gone
from native launches), skippable boot cinematic (click/Enter/Space/Esc),
prefers-reduced-motion honoured end-to-end (attr + media query + countUp
snap; data-motion="full" opts out), uniform card hover physics, value-tick
on instance switch. **tools/uianimtest.mjs committed** (13 assertions).
Both suites green: 13 motion + 16 instance.

### v2.6.0 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.6.0> — fixes
"custom profile fabric 1.8.9 never launches". **Root cause (verified
against the live meta): mainstream Fabric does not support 1.8.9 at all.**
`meta.fabricmc.net/v2/versions/loader/1.8.9/0.15.11/profile/json` answers
HTTP 400; the supported-game list (520 versions) starts at the 1.14 era.
The launcher surfaced that bare 400 — and worse, the wizard *offered*
Fabric × 1.8.9 and **defaulted to 1.8.9**.

- `fabric.rs`: `loader_supports_game()` (releases ≥ 1.14, branch suffixes
  inherit, weekly snapshots 19w+), `unsupported_message()` — one human
  explanation reused by every layer: "Fabric does not support Minecraft
  {v} (1.14 and newer only). Use the VANILLA loader for this version, or
  create a 1.20.4 FABRIC instance for the full Arsex stack."
  `ensure_loader_profile` pre-checks and maps 400/404 to the same words.
  `MOD_TARGET_MC = "1.20.4"` — fabric-api + arsex-mod are 1.20.4-only and
  are skipped on every other version.
- `instance.rs`: creation refuses fabric + unsupported **before any
  download**, same message.
- `pipeline.rs`: loader-only fabric launches on non-1.20.4 emit
  `launch://mod-problem` (visible notice, no silent stack).
- `args.rs`: legacy versions keep `-cp`/`-Djava.library.path` even when a
  loader profile layers a sparse jvm block on top (previously the
  classpath was silently dropped — the JVM could not find the main class).
- Wizard: defaults to **1.20.4**; Fabric disabled on pre-1.14 with a
  tooltip; **Forge/Quilt visibly blocked** (never provisioned — the
  silent-vanilla lie is gone); picking a legacy version falls back to
  VANILLA with a toast.
- Tests: core-launch 59 unit + 9 live (real-meta 1.8.9 refusal; real
  fabric-1.16.5 profile→merge→argv with classpath), pipeline-check 24,
  insttest 21 (wizard gating), core 33, mod 71. CI `33310042734` (main)
  + `33310456910` (tag) green.
- Assets: `arsex.exe` 5,863,936 B · `Arsex.Client_2.6.0_x64-setup.exe`
  2,234,521 B · `arsex-mod-2.5.0.jar` 45,583 B (mod unchanged).
- Vanilla 1.8.9 launches normally (piston-served, verified era test).

### v2.6.1 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.6.1> — **adds the
native bridge. Until this release the packaged app could not install
anything, because no bridge had ever been written between the webview UI
and the Tauri backend.** The user's report — "new instance made but the
MC version does not install, and the fabric loader does not install" —
was the first end-to-end Windows test to surface it. Every backend call
in the UI sits behind a guard (`if (window.__createInstance)` /
`__nativeLaunch` / `__listInstances` / `__scanMods` / …) and nothing
anywhere defined those functions. The Rust commands were registered and
complete the whole time; the guards quietly failed, so the wizard
refused with "needs the desktop app" and LAUNCH ran the scripted
preview sequence while installing nothing. **This also reframes the
original fabric-1.8.9 report: that launch never reached the backend's
HTTP 400 either — the UI was in preview mode.** The v2.6.0 gating is
still correct (verified against the live meta) and still worth having.

- Bridge block at the top of the app script, activates only when
  `window.__TAURI__` exists (static preview keeps honest fallbacks).
  Wires: `list_instances`, `create_instance`, `current_account`,
  `begin_login`, `kill_game`, `scan_mods`, `install_mod`, `toggle_mod`,
  `delete_mod`, `launch_game` (token stays `''` in the webview; Rust
  resolves identities at launch). It must stay ABOVE the boot-time
  calls (`INST.refresh()`, `mmSyncNative()`, `acctRefresh()` run during
  initial script evaluation).
- Events: `launch://stage` → **heroStage + console pct** (heroStage was
  previously fed only by the preview emitter); `instance://stage` →
  wizard overlay; `game://log` → Console via `CON.attach(pid)` on
  handoff; `game://exit`/`game://crash` → exit code + crash line +
  toast; `launch://mod-problem` → WARN line + toast (v2.6.0 emitted
  this into silence).
- **tools/bridgetest.mjs committed** (16 assertions): injects a mock
  `__TAURI__` before load, asserts every payload shape byte-for-byte,
  boot pulls instances/account/mod-scan, live stage events drive both
  the wizard overlay and the hero bar, log lines land, exit code shows,
  backend refusal text renders verbatim, and the bridge stays inert
  without `__TAURI__`.
- All three browser suites green: bridge 16 + instance 21 + motion 13;
  mono-lint clean; zero Rust changes (engine suites run in CI).
- Assets: `arsex.exe` 5,863,936 B · `Arsex.Client_2.6.1_x64-setup.exe`
  2,236,201 B · `arsex-mod-2.5.0.jar` 45,583 B (unchanged). CI
  `33313447135` (main) + `33313908112` (tag) green.
- Session note: the dev workspace snapshot truncated (lost `launcher/`
  and `.git`); the tree was recovered by cloning origin — the repo IS
  the source of truth, exactly why pushes happen before turn end.

### v2.6.2 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.6.2> — fixes
"the fabric loader does not download — creating a 1.21.x instance
always gets stuck at 6%". **6% is exactly the "Installing Fabric
loader" stage.** The loader profile is a single 2.8 KB fetch from
meta.fabricmc.net, previously attempted once with no connect timeout —
a dropped connection or blocked route hung up to the 120 s total
timeout, and because a failed LAUNCH never reset the hero bar, it froze
at 6% forever with the reason buried in the Console page. (The endpoint
itself is healthy: 200 for 1.16.5/1.20.4/1.21.1/1.21.4, verified.)

- `fabric.rs` `fetch_profile_with_retry`: 3 attempts, 800 ms/2.5 s
  backoff, retries connection errors + 5xx + 429; 400/404 still maps to
  the human unsupported message; final failure names the host and
  suggests connection/proxy checks.
- Both HTTP clients (launch + creation) got a 15 s `connect_timeout`.
- **The Fabric stack now installs DURING instance creation** with real
  stages in the wizard overlay (loader profile → fabric-api → Arsex
  mod at 96–97%), instead of being deferred to first launch — the
  deferral is what made "the loader does not download" look like a
  creation bug. First launch re-verifies the warm cache in
  milliseconds. `provision_fabric` takes a `base_pct` so its stages
  position on either caller's progress scale (launch 6.0, creation
  96.0).
- Frontend `heroReset()`: a failed launch clears the hero bar (was:
  frozen at the last stage forever) and toasts the full error text.
- Tests: live `fabric_profile_for_1214_resolves_from_real_meta` (real
  fetch + cache); bridgetest **18** (+ failed-launch resets bar, ERROR
  visible); insttest 21; uianimtest 13; core-launch 59 unit + **10**
  live; pipeline-check rebuilt at **27** (the workspace truncation had
  gutted it — tauri-stub/tauri-macros sources rewritten, repo
  game/auth copied in); mono-lint clean.
- Assets: `arsex.exe` 5,864,448 B · `Arsex.Client_2.6.2_x64-setup.exe`
  2,236,452 B · `arsex-mod-2.5.0.jar` 45,583 B (unchanged). CI
  `33315058821` (main) + `33315510502` (tag) green.
- Note: if a user's network blocks meta.fabricmc.net entirely, creation
  now fails loudly at ~96% with the named host after ~20 s (3 tries +
  backoff), and the profile JSON is cached after the first success so
  later installs never re-fetch it.

### v2.6.3 — released 2026-08-30

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.6.3> — closes
the two honesty gaps left on the launch path.

1. **Owners launch with a REAL Microsoft session.** Until now only the
   demo tier resolved a session in Rust; owners kept the `''` token the
   webview passes — singleplayer limped, every server join died with an
   invalid session. `resolve_launch_identity` now runs `login_silent`
   (MSA refresh → Xbox → Minecraft) for BOTH tiers, rotates and
   persists the refresh token, and returns `Owner(Session)` /
   `Demo(Session)`. Entitlement drift in either direction refuses with
   "sign in again" instead of half-launching. `Unknown` (no matching
   account) refuses outright: no unauthenticated launches, the demo
   tier is the account-less path. **Unverified against live MS (no test
   credentials) — the user's signed-in launch is the gate.**
2. **"Copy current config" is real.** The wizard toggle existed,
   defaulted ON, and was silently ignored. The create payload now
   carries `copy_config_from` (active instance slug) and creation
   clones that instance's `config/` after downloads succeed —
   merge-not-mirror (`copy_tree`), missing source skipped with a
   visible note, never a failure.

Tests: pipeline-check 23 lib (`copy_tree` merge/overwrite/skip; owner
with a broken vault is REFUSED, not emptied) + 5 auth; bridgetest 19
(`copy_config_from` in the payload); insttest 21; uianimtest 13;
mono-lint clean. The Windows CI job compiled the new `main.rs` match
(the one file the sandbox cannot build). Assets: `arsex.exe` 5,871,616 B
· `Arsex.Client_2.6.3_x64-setup.exe` 2,237,976 B · `arsex-mod-2.5.0.jar`
45,583 B (unchanged). CI `33316295905` (main) + `33316748547` (tag)
green. **Behaviour change to tell the user about: LAUNCH without any
signed-in account is now refused with a clear message** (was: an
empty-token launch).

### v2.7.0 — released 2026-09-01

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.7.0> — four
improvements, each closing a gap between what the backend could do and
what the app exposed. Three Rust commands were registered-but-dead
wiring until now (`delete_instance`, `list_versions`, `open_log_dir`).

1. **Instance management** (was: none — instances were un-removable).
   MANAGE button beside NEW INSTANCE opens a modal for the active
   instance: memory right-sizing (2–16 GB tiles → new
   `set_instance_memory` command) and delete with a two-step confirm
   (first click arms for 3.2 s; deleting removes isolated worlds).
2. **Java preflight.** Every launch verifies the Java binary exists
   and parses `java -version` (modern 17/21, legacy 1.8→8; a "1.21"
   banner is refused, not guessed — caught by its own test) BEFORE the
   handoff. Missing/too-old JDK fails with words naming the requirement
   and adoptium.net.
3. **Live version list.** The wizard's curated tiles claimed "1.21.4 =
   Latest release" months stale. ALL RELEASES expander lists real
   releases from `list_versions` (real Mojang manifest), and the Fabric
   gate is now a predicate mirroring Rust `loader_supports_game` (≥1.14,
   snapshots 19w+) instead of a lookup table — 1.21.1 works, 1.12.2
   stays gated, correct for any future version.
4. **Honest settings + LOG FOLDER.** Removed the fabricated "Auto
   Update" and "Crash Recovery" rows; Console gains LOG FOLDER
   (`open_log_dir`) next to SAVE LOG.

Bridge additions: `__deleteInstance`, `__setMemory`, `__openLogs`,
`__listVersions`. Tests: pipeline-check 26 lib + 5 auth; bridgetest
**27** (manage save payload, double-confirm delete, live list + fabric
predicate both directions, open_log_dir); insttest 21; uianimtest 13;
mono-lint clean. Turn-start hygiene: snapshot had dropped the exec bit
on `mod/gradlew`/`mod/run-tests.sh` — restored via
`git update-index --chmod=+x` (watch for this after every workspace
truncation). Assets: `arsex.exe` 5,878,784 B ·
`Arsex.Client_2.7.0_x64-setup.exe` 2,241,027 B · `arsex-mod-2.5.0.jar`
45,583 B (unchanged). CI `33484244545` (main) + `33485040829` (tag)
green.

### v2.7.1 — released 2026-09-01

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.7.1> — the
download engine under the pre-existing creation/launch functions was
rebuilt: parallel, retried, warm-start verified. No new features.

- `install::fetch`: per-file retry (3 attempts, 400 ms/1.2 s backoff)
  on connection errors, 5xx and 429. Until now ONE dropped connection
  in a ~4000-file asset pass failed the whole creation — and creation
  cleanup then deleted everything. Hash mismatches are never retried
  and never written (corruption ≠ transience).
- `install::fetch_all`: six parallel workers via `std::thread::scope`;
  first error wins and stops new work; progress throttled to every 25
  files, byte-based fraction. Both sequential loops (`pipeline.rs`
  `run_downloads`, `instance.rs` `download`) are thin wrappers over it
  now — creation goes from one connection to six.
- **Warm launches.** Assets are content-addressed, so existence+size
  is sound while the index JSON stays SHA-1 verified. A fully
  successful pass writes `indexes/<id>.ok`; stamped runs plan by size
  instead of re-hashing ~500 MB per launch (stage detail says
  "warm pass · sizes only"). A changed asset has a different hash →
  different path → real download; the fast path cannot go stale.
- Tests: core-launch **64** unit (+5 on a local `tiny_http` server —
  retry-then-land with exact attempt count, mismatch never
  retried/written, parallel totals, poison URL named in the error,
  fast/slow plan agreement; `tiny_http` is now a core-launch
  dev-dependency) + 10 live; pipeline-check 26+5; bridgetest 27;
  mono-lint clean. First pipeline-check auth run after a rebuild can
  show a one-off flake — re-run passed 4× deterministically.
- Assets: `arsex.exe` 5,882,368 B · `Arsex.Client_2.7.1_x64-setup.exe`
  2,243,658 B · `arsex-mod-2.5.0.jar` unchanged. CI `33487397641`
  (main) + `33488261358` (tag) green.

### v2.8.0 — released 2026-09-02

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.8.0> —
**Performance mode**: every honest FPS lever a launcher has, behind an
opt-out that is ON by default for new instances and per-instance
toggleable in MANAGE. A launcher cannot multiply raw render FPS and
nothing claims to; this removes the stalls around the renderer.

- JVM extras (deduped against manifest/loader flags; vanilla 1.20.4
  ships NO GC flags — verified live): `-XX:G1HeapRegionSize=32m`,
  `-XX:+DisableExplicitGC` (no more System.gc() full-GC stalls from
  mods), `-XX:+AlwaysPreTouch` on heaps ≥ 4 GB.
- Fixed heap on perf launches: `-Xms == -Xmx` (no resize collections).
- Windows process priority ABOVE_NORMAL, OR'd into the existing
  CREATE_NO_WINDOW creation_flags (the call REPLACES — two calls would
  have dropped the no-console flag).
- options.txt seed at creation for perf instances:
  `maxFps:260`, `enableVsync:false`, `pauseOnLostFocus:false` — only
  when no options.txt exists; copied/hand-tuned files are never
  clobbered.
- Existing instances: `#[serde(default)] perf: false` — registry from
  older builds loads unchanged (test covers the missing-key path);
  flip via MANAGE (`set_instance_perf`). JVM + priority extras apply
  on every launch of a perf instance; the options seed is
  creation-only.
- Checked the mod's real modules first (Coordinates, CPS, FPS counter,
  Fullbright, Zoom): NO performance modules exist, none claimed.
- Tests: core-launch 65 unit + 10 live; pipeline-check 28 lib + 5
  auth; bridgetest 31; insttest 21; uianimtest 13; mono-lint clean.
  Assets: `arsex.exe` 5,883,904 B · setup 2,244,090 B · mod unchanged.
  CI `33601030627` (main) + `33601822820` (tag) green.

### v2.9.0 — released 2026-09-02

<https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.9.0> — UI /
motion / background pass, strictly inside the standing rules (greyscale
only, ZERO border-radius — verified, the mist masses are radial-gradient
fades not shapes; hand-authored SVG only; reduced-motion honoured).

- **Background**: ink field (three hand-placed masses drifting on
  independent 47s/71s/89s alternate loops + dark bottom wash, GPU
  transforms only), hand-drawn sumi-e contour-line SVG at the bottom
  (inline data-URI, 5% opacity), and rAF pointer parallax — the field
  eases a few px against the cursor. A reduced-motion switch MID-DRIFT
  freezes the field on the next frame (tick() re-checks the attr).
- **Micro-interactions**: press feedback on every actionable element
  (`.pressing` class mirrors `:active` because some webviews never
  activate the pseudo-class), `:focus-visible` hairline outline for
  keyboard users, instance chips wave in (40ms stagger).
- Tests: uianimtest **19** (+ink layers/drift keyframe, parallax +
  freeze, stagger delays, press held/released, focus-visible rule).
  **Test trap recorded: the boot screen is a z-500 full-screen overlay —
  skip it before any pointer-driven assertion.** insttest 21,
  bridgetest 31, mono-lint clean.
- Assets: `arsex.exe` 5,888,000 B · setup 2,246,120 B · mod unchanged.
  CI `33607057131` (main) + `33607929013` (tag) green.

### Still open

- In-game use of the menu/modules by a human (see above).
- `ARSEX_AZURE_CLIENT_ID` secret; EV cert. Both the user's to resolve.
- **Owner launches still take the token from the webview call** (`''` today):
  `login_silent` exists and the demo tier uses it, but owning accounts do
  not yet resolve a real session at launch. The honest fix is to route owners
  through `resolve_launch_identity` too — the plumbing now exists on the
  demo path.

---

## Constraints the user has set — do not violate

1. **No cracked/offline accounts.** Real Microsoft OAuth only. The user asked
   for an offline launch path; I declined, citing their own commitments in
   `README.md:269`, `docs/ARSEX_SPEC.md:130` and `auth/demo.rs`. `can_launch()`
   is `const fn -> false`; `DemoProfile` has no token field by construction.
   - **The honest alternative, agreed but not yet built:** Minecraft's official
     free demo. A real MSA login on an account that has *not* bought the game
     returns a genuine session with no entitlement; pass `--demo` and you get
     real singleplayer with a 5-day world. `args.rs:68` already emits
     `("is_demo_user", false)` — that is the hook point.
2. **Strict monochrome.** `#000000`, `#F5F5F5`, `#FFFFFF`, greyscale only. No
   colour accents anywhere, including error states. `node tools/mono-lint.mjs`
   enforces this; keep it passing.
3. **No rounded corners.** The user called them "stupid". All radii are 0;
   chamfers + registration ticks instead. Never reintroduce `border-radius`.
4. **Lunar Client layout structure** (horizontal top bar, centred hero LAUNCH,
   news + friends row) with the Arsex visual language.
5. **No AI-generated images for background scenery** — hand-authored SVG only.
   (Character *figures* were the exception, and both were removed anyway.)
6. **Never give edit instructions by line number.** Deliver whole files or a
   script. Line-number edits already broke two CI rounds.
7. **Token hygiene.** Pipe every push through
   `sed 's/ghp_[A-Za-z0-9]*/[REDACTED]/g'`, de-tokenise the remote afterwards,
   delete the clone. Advise revocation — the token has been posted in plaintext
   repeatedly and GitHub's scanner may already have caught it.

---

## Traps that already cost time — do not rediscover these

**Sandbox durability.** The sandbox has wiped `~/.cargo`, `~/node_modules`,
`mod/src/`, `launcher/dist/`, `target/`, `/tmp`, and even chmod bits on
`/home/user/jdk17/bin/*` **mid-session, more than once**. If a file you wrote
an hour ago is missing, that is why. Re-clone from GitHub to recover; it is
the durable copy. Symptom of the JDK losing its `+x` bits: `run-tests.sh`
silently falls back to Java 11 and reports 24 bogus syntax errors on switch
expressions. Fix: re-extract the JDK (a `chmod` alone is not enough — files
get truncated too).

**Do not `cargo check` `launcher/src-tauri` locally.** Takes >1700 s and OOMs
the GUI link at 1 GB. To type-check a module against real dependencies, copy it
into a throwaway crate with the tauri surface stubbed — see how `instance.rs`
was verified (7 tests, 33 s).

**GitHub artifacts cannot be downloaded anonymously** — `401` even on a public
repo. That is why tagging matters: release assets *are* anonymous.
**Never invent artifact IDs.** I did, sent the user to a 404, and they
reasonably concluded the build was broken. Always query
`/actions/runs/<id>/artifacts` or the releases API, then verify with
`curl -sIL` before handing over a link.

**Puppeteer** needs `libnspr4 libnss3 libasound2t64 libatk-bridge2.0-0
libgtk-3-0 libgbm1` and a `python3 -m http.server` in `prototype/`. Test
scripts must live under `/home/user`.

**Frontend sync.** `prototype/arsex.html` is the source; `launcher/dist/` is
generated and gitignored. Run `node tools/sync-frontend.mjs` after any
prototype edit — the IPC bridge is injected at sync time and is *not* in the
prototype, which always falls back to browser stubs.

**Other landmines:** `python3` on Windows is a Store stub, use `py`. Write
UTF-8 without BOM. Don't generate JS containing backticks from a JS template
literal. A Python patch script with multiple `assert`s writes nothing if a
later assert fails. `clip-path` removes the border along the cut diagonal —
chamfered elements need `.ch`.

---

## Verification gate — all of this must stay green

```bash
bash core/run-tests.sh                    # 33  Java core
bash mod/run-tests.sh                     # 71  mod core (needs JDK 17)
cd mod && ./gradlew build                 # the REAL compile (network + JDK 17)
cd launcher/core-launch && cargo test     # 55  launch engine
node tools/mono-lint.mjs prototype/       #     colour gate
node tools/mono-lint.mjs launcher/dist/
node tools/sync-frontend.mjs              #     regenerate dist
```

Last full green at v2.5.0: 33 + 71 + 51 (+6 live Mojang), real gradle build
green, `pipeline-check` scratch crate (tauri-stubbed src-tauri modules) 12/12
in both cfg states, mono-lint clean.

NOTE: the Puppeteer suites (`wiztest/accttest/contest/uxtest/mmtest/nav/
scenetest/demotest.mjs`) lived only in the old sandbox at `/home/user` and
were never committed, so they are gone. The prototype changed only by a
version string; recreate the suites before trusting future UI changes.

---

## Key facts worth not re-deriving

- **IPC surface, 18 commands.** `launch_game{instance,version,player,uuid,token,memory,java}->pid`
  · `kill_game` · `game_running` · `scan_mods{instance,loader}` ·
  `install_mod{instance,source}` · `toggle_mod{path,enabled}` ·
  `delete_mod{path}` · `list_versions` · `set_demo{on}` · `open_log_dir` ·
  `begin_login` · `begin_demo{nickname}` · `current_account` · `logout{uuid}` ·
  `create_instance{req}` · `list_instances` · `delete_instance{slug}` ·
  `check_instance_name{name}`
- **Events.** `launch://stage {key,label,pct,detail}` · `launch://mod-problem` ·
  `instance://stage` · `game://log` · `game://exit` · `game://crash` · `app://ready`
- **Paths.** `%APPDATA%\Arsex` config/tokens/profiles ·
  `%LOCALAPPDATA%\Arsex` caches/libraries/assets/instances/logs/crashes.
  Instance slugs `[A-Za-z0-9_-]{1,64}`; `slugify()` in `instance.rs` and the JS
  copy in `arsex.html` **must stay identical** or names collide silently.
- **Tauri v2.** ACL denies all IPC unless granted in
  `src-tauri/capabilities/default.json` (16 permissions, `windows:["main"]`,
  `core:default` required). `"withGlobalTauri": true` must stay in
  `tauri.conf.json`. Failures are silent at runtime, never at compile time.
- **Known blockers.** No `ARSEX_AZURE_CLIENT_ID` secret set → sign-in cannot
  complete in shipped builds (CI uses an all-zeros fallback). No EV cert →
  SmartScreen warns. Both are the user's to resolve.

---

## Suggested order of work

1. `settings.gradle` + Gradle wrapper + icon → get `./gradlew build` to run.
2. Fix the real compile errors, especially the three mixins.
3. **Actually launch Minecraft and confirm Right Shift opens the menu.** No
   further claims about the mod working until this happens.
4. CI job: build the jar, attach it to releases.
5. Auto-install the jar into instances from the launcher.
6. ~~Official `--demo` support~~ — DONE in v2.5.1 (see above).
7. Tag `v2.5.0`, verify links anonymously, hand over.
