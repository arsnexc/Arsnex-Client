# Getting an `.exe`

Three routes. Pick by what you have available.

| Route | Needs | Time | Best for |
|---|---|---|---|
| **A. GitHub Actions** | A GitHub account | ~20 min, unattended | No Windows machine; giving builds to testers |
| **B. Your own Windows PC** | Win 10/11 + ~8 GB disk | ~25 min first build | Fast iteration while developing |
| **C. Signed release** | Route A or B + EV certificate | +1 day setup | Public launch without SmartScreen warnings |

---

## Route A — GitHub Actions (recommended)

The repo already contains `.github/workflows/build.yml`. Pushing the code is
all that is required.

```bash
cd arsex-client
git init                              # already done if you copied the workspace
git add -A
git commit -m "Arsex Client"
git branch -M main
git remote add origin https://github.com/<you>/arsex-client.git
git push -u origin main
```

Then on github.com:

1. Open the **Actions** tab. A run named *Build Windows .exe* starts automatically.
2. Wait for the green tick (~20 min; later runs are faster thanks to caching).
3. Open the run and download from **Artifacts**:
   - `arsex-exe` — the standalone `arsex.exe`
   - `arsex-installer` — the NSIS setup executable

### Add your Azure client ID

Without it the build still succeeds, but Microsoft sign-in will fail at runtime
because the ID is compiled in via `env!()`.

**Settings → Secrets and variables → Actions → New repository secret**

- Name: `ARSEX_AZURE_CLIENT_ID`
- Value: your application (client) ID from
  [portal.azure.com](https://portal.azure.com) → App registrations

The app registration must be a **public client** with a **loopback redirect URI**
(`http://127.0.0.1`), and separately approved by Mojang for Minecraft API
access — otherwise leg 4 of the auth chain returns 403.

### Hand builds to testers

```bash
git tag v2.4.1
git push origin v2.4.1
```

This publishes a **public GitHub Release** with both executables attached, so
testers download from a normal release page instead of needing CI access.

---

## Route B — Build on Windows yourself

**Prerequisites**

1. [Visual Studio Build Tools](https://visualstudio.microsoft.com/downloads/) —
   select the **Desktop development with C++** workload. This provides
   `link.exe`; nothing links without it.
2. [Rust](https://rustup.rs)
3. [Node.js 20+](https://nodejs.org)

**Build**

```powershell
cd arsex-client
$env:ARSEX_AZURE_CLIENT_ID = "<your-azure-app-id>"
pwsh tools\build.ps1
```

The script gates on the client ID, the monochrome lint, the launch-engine tests
and the Java harness *before* compiling, then reports artifact sizes against the
12 MB budget.

**Output**

```
launcher\src-tauri\target\x86_64-pc-windows-msvc\release\arsex.exe
launcher\src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\*-setup.exe
```

Useful flags: `-SkipTests` to skip the gates, `-Sign` to authenticode-sign.

### If it fails

| Symptom | Cause |
|---|---|
| `link.exe not found` | C++ workload missing, or not in a Developer PowerShell |
| `ARSEX_AZURE_CLIENT_ID is not set` | Set the env var in the *same* shell session |
| `frontendDist ... doesn't exist` | Run `node tools\sync-frontend.mjs` |
| `error: linker ... ring` | You are cross-compiling from Linux — use Route A |

---

## Route C — Code signing

Unsigned builds trigger **SmartScreen** on first run: *"Windows protected your
PC"*. Testers can click *More info → Run anyway*, which is fine for a
pre-release but not for a public launch.

To remove it you need an **EV code-signing certificate** (~$350/yr from
DigiCert, Sectigo, etc.). Two things people usually get wrong:

- An **OV certificate does not grant instant SmartScreen trust.** Only EV does.
  OV builds reputation slowly, over many downloads.
- **Since June 2023, signing keys must live in FIPS 140-2 Level 2 hardware.**
  You cannot keep a `.pfx` on disk or in a CI secret. Automated signing requires
  a cloud HSM (Azure Key Vault, DigiCert KeyLocker, SSL.com eSigner).

With the certificate installed locally:

```powershell
$env:ARSEX_CERT_THUMBPRINT = "<sha1-thumbprint>"
pwsh tools\build.ps1 -Sign
```

The script injects the thumbprint into `tauri.conf.json`, builds, then verifies
the signature with `Get-AuthenticodeSignature` and fails if it is not `Valid`.

---

## What "working" means today

Verified here:

- Launch engine: **43 unit tests + 6 live Mojang API tests**, covering 1.8.9
  through the current release
- Tauri app: **10 tests**, `cargo check` clean with the pipeline wired in
- Java core: **33 tests**; UI suites: 100+ Puppeteer assertions

Not yet verified, because it needs a real Windows box with Java and a real
Microsoft account:

- The final JVM spawn and in-game session

Everything up to that point — manifest resolution, library and asset download
with SHA-1 verification, natives extraction, classpath assembly, argv
construction, mod metadata parsing — is tested against real Mojang data.
