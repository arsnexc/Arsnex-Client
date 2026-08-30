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
