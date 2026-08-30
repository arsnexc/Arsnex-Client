# ARSEX CLIENT — 斬
### Production Specification · Windows 10/11 x64 · v2.5.0

---

## 0. Reality Check (read first)

This document is a **buildable blueprint**, not a shipped binary. Four things must come from you, not from a spec, before an `.exe` can legally and technically exist:

| Blocker | What it is | How to clear it |
|---|---|---|
| **Azure client ID** | Microsoft OAuth app registration | Register at portal.azure.com → App registrations. Then apply for Minecraft API access via the Mojang/Microsoft developer form. Without approval, `api.minecraftservices.com` returns 403 for your client ID. |
| **Game code** | You may not redistribute Minecraft or its assets | Ship a **launcher + mod layer**. The launcher downloads official artifacts from Mojang's CDN using the user's own entitlement; your mods load via Fabric/Forge. |
| **Code-signing cert** | Unsigned `.exe` gets SmartScreen-blocked | OV cert (~$200/yr) builds reputation slowly; **EV cert** (~$350/yr, hardware token) gets instant SmartScreen trust. Buy from DigiCert/Sectigo. |
| **EULA compliance** | Minecraft EULA forbids charging for the game, and most servers ban combat automation | Ship only QoL/visual/HUD modules. **No killaura, no reach extension, no flight.** The 63 modules in this spec are deliberately all server-legal. |

Everything below is real and implementable once those are in hand.

---

## 1. Identity

**Name** Arsex Client · **Mark** 斬 (*zan* — "to cut") · **Motto** 一期一会

The aesthetic is not "dark mode." It is **sumi-e** — Japanese ink-wash painting. That means: enormous negative space, one decisive stroke instead of five hesitant ones, asymmetric balance, and value contrast doing all the work colour normally does.

**The monochrome lock is enforced, not requested.** Design tokens are authored in HSL with hue and saturation channels stripped at build time. A CI lint (`tools/mono-lint.js`) walks the compiled CSS/shader output, parses every colour literal, and **fails the build** if any resolves to saturation > 0. It is impossible to merge colour into `main`.

### Palette

| Token | Hex | Use |
|---|---|---|
| `ink-000` | `#000000` | Void, app background |
| `ink-050 → 140` | `#050505 → #141414` | Chrome, panel bases |
| `ink-1c0 → 3a0` | `#1c1c1c → #3a3a3a` | Raised surfaces, hairlines |
| `ink-5a0 / 8c0` | `#5a5a5a / #8c8c8c` | Muted + secondary text |
| `paper` | `#F5F5F5` | Primary text, active fills |
| `steel` | `#FFFFFF` | Edge highlights, blade glints only |

White is **rationed**. Pure `#FFFFFF` appears only on a blade edge, an active toggle, and the boot animation's cutting tip. If everything glows, nothing is sharp.

### Typography
- **UI** Inter var / Segoe UI Variable — 200 for display, 500–600 for labels
- **Numeric & technical** JetBrains Mono, tabular figures locked on
- **Accent glyphs** Yu Mincho / Noto Serif JP — used at ≤5% opacity as watermarks, never as functional text an English speaker must read

---

## 2. Motion System

One curve family across the entire product. Inconsistent easing is the single biggest tell of amateur UI.

```
--e-out   cubic-bezier(.16, 1, .3, 1)      expo-out — the house curve, 90% of transitions
--e-io    cubic-bezier(.65, 0, .35, 1)     symmetric, for looping/progress
--e-snap  cubic-bezier(.34, 1.56, .64, 1)  ~4% overshoot, toggles and knobs only

--t-fast  140ms   hover, press
--t-mid   280ms   toggles, panels
--t-slow  550ms   page transitions
--t-cine  900ms   boot, launch
```

**Rules**
1. Nothing animates longer than 900ms except the boot sequence.
2. Every interactive element responds within **one frame** (<16ms) — press feedback is never queued behind an async call.
3. Transforms and opacity only. No animating `width`, `top`, `box-shadow` spread, or `filter` in hot paths.
4. Overshoot is reserved for toggles. Page transitions never bounce — bounce reads as playful, and Arsex is not playful.
5. `Animation Speed` slider (0–200%) multiplies every duration token at runtime. Reduced-motion collapses all to 1ms.

**Signature animations**
- **Unsheathing** — boot progress is a blade of light drawn left→right with a glowing tip, 2.6s on `--e-io`. Progress bars that look like progress bars are a wasted opportunity.
- **Ink bloom** — the 斬 mark enters at `blur(18px) scale(1.22)` and resolves to sharp over 1.5s, mimicking ink settling into paper.
- **The cut** — enabling a module draws a 2px white slit down the card's left edge, expanding from centre outward in 400ms.
- **Blade sweep** — buttons pass a 105° specular band across their surface on hover, 750ms.

---

## 3. Architecture

Three processes, deliberately separated so a crashed UI never takes down a running game.

```
┌─────────────────────────────────────────────────────────┐
│  Arsex.exe  —  single-file, self-contained, signed      │
│                                                          │
│  ┌────────────────────┐      ┌────────────────────────┐ │
│  │  SHELL (Rust)      │◄────►│  UI (WebView2)         │ │
│  │  Tauri 2.x         │ IPC  │  SolidJS + Vite        │ │
│  │  ~4 MB             │      │  no runtime shipped    │ │
│  │                    │      │  (uses OS Edge)        │ │
│  │ · OAuth + PKCE     │      │ · sub-ms fine-grained  │ │
│  │ · DPAPI vault      │      │   reactivity           │ │
│  │ · Process spawn    │      │ · GPU-composited only  │ │
│  │ · Delta updater    │      └────────────────────────┘ │
│  │ · Crash handler    │                                  │
│  └─────────┬──────────┘                                  │
└────────────┼─────────────────────────────────────────────┘
             │ spawns, isolated
   ┌─────────▼──────────────────────────────────┐
   │  JVM  —  Minecraft + Fabric Loader         │
   │  arsex-core.jar  (Mixin 0.8.x)             │
   │  · module registry   · HUD compositor      │
   │  · in-game Click GUI · IPC back to shell   │
   └────────────────────────────────────────────┘
```

### Why this stack

**Tauri over Electron.** Electron ships a 150 MB Chromium copy and idles at ~180 MB RAM. Tauri uses the WebView2 runtime already present on every Windows 10 20H2+ machine: **~9 MB installer, ~55 MB idle**. For a client whose entire pitch is performance, shipping a browser alongside the game is self-defeating.

**SolidJS over React.** No virtual DOM, no reconciliation pass. Signals write directly to the node. On a 120Hz panel the difference between a 3ms and a 0.4ms update is the difference between "smooth" and "buttery."

**WebView2 fixed-version fallback.** Detect at launch; if the Evergreen runtime is missing (rare, but LTSC/N-editions), the bootstrapper pulls it silently rather than erroring out.

### Directory layout
```
%LOCALAPPDATA%\Arsex\
├── vault.dat            DPAPI-sealed refresh tokens
├── profiles\*.arsex     configs (JSON + zstd)
├── instances\<uuid>\    isolated .minecraft roots
├── logs\                rolling, 10 MB × 5, PII-scrubbed
└── crash\               minidumps + state snapshots
```

---

## 4. Authentication — Real Microsoft OAuth 2.0

Five legs. No shortcuts, no offline mode compiled in anywhere.

```
1. AUTHORIZE   login.microsoftonline.com/consumers/oauth2/v2.0/authorize
               response_type=code
               code_challenge_method=S256          ← PKCE mandatory
               redirect_uri=http://127.0.0.1:<ephemeral>
               scope=XboxLive.signin offline_access
               state=<128-bit CSRF nonce>

2. TOKEN       POST /token  →  MSA access_token + refresh_token

3. XBL         user.auth.xboxlive.com/user/authenticate
               → XBL token + userHash (uhs)

4. XSTS        xsts.auth.xboxlive.com/xsts/authorize
               → XSTS token
               ⚠ handle XErr 2148916233 (no Xbox account)
                          2148916238 (child account, needs family)

5. MINECRAFT   api.minecraftservices.com/authentication/login_with_xbox
               → MC access_token (24h)
               GET /entitlements/mcstore  → verify ownership
               GET /minecraft/profile     → UUID + skin
```

**Security posture**
- The auth window is a **system browser tab**, not an embedded WebView. Embedded auth UIs are a credential-phishing pattern and Microsoft actively discourages them. Arsex never sees a password, and cannot.
- Redirect is loopback on an ephemeral port with `state` + PKCE verifier both checked. Rejects any callback that doesn't match.
- Refresh tokens → `CryptProtectData` (DPAPI, `CRYPTPROTECT_UI_FORBIDDEN`, user-scope, per-install entropy blob). Copying `vault.dat` to another machine or user yields nothing.
- Access tokens live in memory in a `Zeroizing<String>`, wiped on drop and on exit.
- Multi-account: each entry stores only `{ uuid, name, sealed_refresh }`. Switching = silent refresh, no re-login, typically <400ms.

---

## 5. Feature Registry — 63 Modules

Every module is a `Module` trait impl: `id`, `category`, `default_keybind`, `settings: Vec<Setting>`, `on_enable/on_disable`, `on_render(ctx)`. Adding one is a single file plus a registry line.

**All 63 are server-legal QoL.** No combat automation.

### Performance (7)
1. **FPS Boost Suite** — occlusion queries, draw-call batching, chunk mesh caching, mipmap tuning
2. **Entity Culling** — frustum + occlusion skip for hidden entities
3. **Dynamic FPS** — throttle to 10fps unfocused, 30fps minimised
4. **Chunk Optimiser** — async mesh building off the render thread
5. **Memory Cleaner** — GC hint + native heap trim, with before/after readout
6. **Smart Render Distance** — auto-scales to hold a target framerate
7. **Shader Cache Warmer** — precompiles pipelines during the loading screen

### Visual (16)
8. **Fullbright** — uniform luminance without gamma clipping
9. **Zoom** — cinematic eased zoom with inertia and adjustable sensitivity scaling
10. **Free Look** — decouple camera yaw from movement direction
11. **Free Cam** — untethered spectator camera (singleplayer/spectator only)
12. **Motion Blur** — per-object velocity blur, tunable shutter angle
13. **Custom Sky** — ink-wash gradient skybox, time-of-day driven
14. **Custom Clouds** — volumetric monochrome clouds, density slider
15. **Better Animations** — rebuilt player/item/block rigs
16. **Smooth Camera** — cubic damping on look input
17. **Crosshair Designer** — shape, gap, thickness, outline, dynamic spread
18. **Hit Effects** — ink-splatter impact particles
19. **Particle Editor** — rate, size, lifetime, monochrome luminance ramp
20. **Damage Indicators** — floating numerals with rise-and-fade
21. **Hitbox Viewer** — hairline bounds, no fill
22. **Nametag Customiser** — scale, opacity, font, distance fade
23. **Shader Support** — Iris-compatible; two bundled monochrome performance shaders

### HUD (14)
24. **Target HUD** — health, armour, distance, ping, animated bar
25. **Keystrokes** — animated keys + mouse with CPS overlay
26. **CPS Counter** — L/R split, rolling 1s window
27. **FPS Counter** — with 1%-low frametime graph
28. **Coordinates** — XYZ, biome, facing, chunk-relative
29. **Potion Effects** — icons, timers, pulse under 10s
30. **Armour Status** — durability arcs
31. **Inventory HUD** — hotbar mirror
32. **Player List** — sortable tab overlay
33. **Minimap** — vector-rendered, ink-styled, square or circle
34. **Waypoints** — beacons, auto death-point, share codes
35. **Radar** — entity blips with threat weighting
36. **Reach Display** — rolling average of your own reach
37. **Performance Overlay** — CPU, GPU, RAM, draw calls, chunk updates

### Movement (5)
38. **ToggleSprint** · 39. **ToggleSneak** (edge-safe) · 40. **AutoSprint** · 41. **Inventory Walk** · 42. **Sprint Reset Visualiser** (W-tap timing feedback)

### Interaction (2)
43. **Fast Place** — removes artificial placement cooldown
44. **Fast Break** — optimised break-packet cadence

### Social (8)
45. **Auto GG / GG+** — configurable, per-server templates
46. **Chat Filters** — regex mute, spam collapse, duplicate merge
47. **Chat Themes** — monochrome skins, timestamps, opacity
48. **Friend System** — tags, highlights, cross-server presence
49. **Staff Detector** — flags known staff and vanish patterns
50. **Discord RPC** — server, mode, party, elapsed
51. **Voice Chat Bridge** — Simple Voice Chat compatible
52. **Party Finder** — in-client LFG

### Utility (6)
53. **Screenshot Tools** — instant upload, watermark toggle, region crop
54. **Replay Recorder** — timeline scrub, camera paths, export
55. **Resource Pack Manager** — live reload, ordering, per-profile sets
56. **Multi-Instance** — isolated sandboxed game roots
57. **In-Game Browser** — sandboxed, no plugins, greyscale-filtered
58. **Server Scanner** — MOTD, ping, player sampling

### System (5)
59. **Profiles** — save/load/export/share as `.arsex`
60. **Cloud Sync** — E2E encrypted (key derived from MSA UUID + local secret; the server stores ciphertext it cannot read)
61. **Auto Update** — bsdiff delta patching, staged rollout rings, atomic swap with rollback
62. **Crash Recovery** — 30s state snapshots, minidump capture, guided restore
63. **Theme Editor + Accessibility** — luminance-only authoring; high contrast, reduced motion, HUD scaling, colourblind-irrelevant by construction

---

## 6. UI/UX Detail

### Launcher
Sidebar (236px) → Home · Modules · HUD Editor · Accounts · Cosmetics · Packs · Settings. Active item inverts to a solid paper fill — the strongest contrast in the app, so you always know where you are.

**Home** — a hero LAUNCH card with version chips and live telemetry tiles (FPS, memory, playtime, frametime) that count up on mount with a quartic ease. Below, six quick-toggle module cards.

**Modules** — search + 9 category chips + responsive card grid. Filtering re-runs the stagger animation with a 14ms-per-card delay capped at 300ms, so a 4-item result set snaps and a 63-item set cascades.

**HUD Editor** — 16:9 live preview with draggable elements, magnetic 8/16px snap grid, and a settings stack below.

### In-Game Click GUI
`RSHIFT` opens a backdrop-blurred overlay with draggable category windows. Each row toggles with a full colour inversion. Windows remember position per profile. `ESC` closes.

### Micro-interactions
| Element | Behaviour |
|---|---|
| Button hover | `translateY(-2px)` + specular sweep + soft outer glow |
| Button press | `scale(.975)`, 140ms |
| Toggle | knob travels on `--e-snap`; **stretches to 20px mid-travel** and settles to 15px — a squash-and-stretch trick that reads as physical weight |
| Nav hover | `translateX(3px)` + icon `scale(1.08)` |
| Card hover | `translateY(-4px)` + white-tinted drop shadow |
| Settings row hover | `padding-left` 20→26px — the row leans toward you |
| Slider | knob grows a 7px halo on hover, scales 1.15 |
| Cursor | 26px ring lerped at 0.18 (trails the pointer) + a 3px dot locked to it exactly. Ring expands to 44px over interactives. |

### Sound
Soft Japanese-inspired palette: *suzu* bell (toggle on), damped wood knock (toggle off), paper slide (page transition), single shakuhachi breath (launch). All ≤120ms, −18 LUFS, three levels: Silent / Soft / Full.

---

## 7. Build & Ship

```bash
# UI
pnpm build                    # Vite → dist/, ~180 KB gzipped

# Shell
cargo tauri build --target x86_64-pc-windows-msvc --release

# Signing (EV token)
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 ^
              /a Arsex.exe
```

**`tauri.conf.json` essentials**
```json
{
  "bundle": { "targets": ["nsis"], "windows": {
    "webviewInstallMode": { "type": "downloadBootstrapper" },
    "nsis": { "installMode": "currentUser" }
  }},
  "app": { "security": { "csp": "default-src 'self'; img-src 'self' data:" }}
}
```

**Cargo release profile** — `opt-level="z"`, `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip=true`.

### Targets
| Metric | Budget |
|---|---|
| Installer size | < 12 MB |
| Cold start → interactive | < 1.2 s |
| Idle RAM (launcher) | < 60 MB |
| UI frame time | < 4 ms @ 144Hz |
| Module toggle → visible | < 16 ms |
| In-game overhead | < 2% frametime |

### Update pipeline
Signed manifest → bsdiff delta → download to `.staging` → verify Ed25519 signature → atomic rename on next launch → auto-rollback if the new build fails to reach the ready state twice.

### Crash handling
Rust panic hook + JVM `-XX:+HeapDumpOnOutOfMemoryError`. Minidump + last 200 log lines + module state → local `crash\`, with an opt-in upload prompt. Logs are PII-scrubbed (tokens, UUIDs, paths regex-redacted) before they ever leave the machine.

---

## 8. Build Order

| Phase | Weeks | Deliverable |
|---|---|---|
| 1 | 1–3 | Tauri shell, design tokens, mono-lint in CI, navigation |
| 2 | 4–6 | Full OAuth chain, DPAPI vault, account switcher |
| 3 | 7–9 | Version manifest, asset downloader, JVM spawn, Fabric injection |
| 4 | 10–14 | Module framework + first 20 modules |
| 5 | 15–18 | HUD compositor, in-game Click GUI |
| 6 | 19–22 | Remaining 43 modules |
| 7 | 23–25 | Profiles, cloud sync, cosmetics |
| 8 | 26–28 | Updater, crash recovery, telemetry |
| 9 | 29–32 | Motion polish pass, sound design, accessibility audit |
| 10 | 33–36 | Signing, SmartScreen reputation, closed beta, launch |

**~9 months, 3–4 engineers.** That estimate is the honest one; treat anything shorter with suspicion.

---

*斬 — one cut, clean.*
