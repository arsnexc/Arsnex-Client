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
- **Launch pipeline** (`launcher/core-launch`, crate `arsex-launch`) — 43 tests,
  tauri-free. Classpath dedupe, natives extraction, SHA-1 verification.
- **CI** — `.github/workflows/build.yml`, three jobs: `verify`, `build`
  (Windows x64), `release` (gated on `refs/tags/v*`). Last green run: **#13**
  (`33239216608`), all three jobs success.

### The gap the user is angry about — and they are right

**The mod does not run in game, and there is no jar to download.**

`mod/` contains ~1,400 lines of Java that unit-test cleanly (71 assertions via
`mod/run-tests.sh`) but that has **never been compiled against Minecraft**.
The harness deliberately compiles only the classes free of Minecraft imports:

```
module/*  config/*  modules/{Zoom,Cps,FpsCounter,Coordinates}.java
```

Never compiled by anything, ever:

```
ArsexMod.java          mixin/GameRendererMixin.java
gui/ClickGui.java      mixin/InGameHudMixin.java
hud/HudRenderer.java   mixin/MouseMixin.java
modules/Fullbright.java
```

Concretely missing before a jar can exist:

| Missing | Why it blocks the build |
|---|---|
| `mod/settings.gradle` | Gradle refuses to configure the project |
| `mod/gradlew` + `gradle/wrapper/` | No wrapper, so no reproducible build |
| `src/main/resources/assets/arsex/icon.png` | `fabric.mod.json` references it |
| A CI job for the mod | Nothing builds or publishes the jar |
| Launcher-side auto-install | Player would have to copy the jar by hand |

`mod/README.md` currently says `./gradlew build` works. **It does not.** Fix
that line or the next person will be misled the same way.

### Unverifiable claims to re-check, not trust

- Mixin targets were written from memory against 1.20.4 Yarn. `InGameHudMixin`
  takes `RenderTickCounter`, which is a 1.20.5+ signature — on 1.20.4 the
  `render` method takes a `float tickDelta`. **This is very likely broken.**
- `Fullbright` calls `mc.options.getGamma().setValue(...)`. `SimpleOption`
  gating may reject values above 1.0 depending on version.
- `mc.options.hudHidden` field name is unverified on 1.20.4.

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
cd launcher/core-launch && cargo test     # 43  launch engine
node tools/mono-lint.mjs prototype/       #     colour gate
node tools/mono-lint.mjs launcher/dist/
node tools/sync-frontend.mjs              #     regenerate dist
node /home/user/wiztest.mjs               # 23  wizard
node /home/user/{accttest,contest,uxtest,mmtest,nav,scenetest,demotest}.mjs
```

Last full green: 33 + 71 + 43 + 7 + 23, mono-lint clean, 18 IPC commands
audited with 0 mismatches and 0 orphans.

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
6. Official `--demo` support (item 1 under Constraints).
7. Tag `v2.5.0`, verify links anonymously, hand over.
