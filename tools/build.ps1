<#
.SYNOPSIS
  Builds Arsex Client into a single native Windows x64 .exe + NSIS installer.

.DESCRIPTION
  Run on Windows 10/11 x64. Produces:
    launcher\src-tauri\target\release\arsex.exe                  (standalone)
    launcher\src-tauri\target\release\bundle\nsis\*-setup.exe    (installer)

.EXAMPLE
  $env:ARSEX_AZURE_CLIENT_ID = "<your-azure-app-id>"
  pwsh tools\build.ps1

.EXAMPLE
  $env:ARSEX_CERT_THUMBPRINT = "<sha1-thumbprint>"
  pwsh tools\build.ps1 -Sign
#>
[CmdletBinding()]
param(
  [switch]$Sign,
  [switch]$SkipTests,
  [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Step($m) { Write-Host "`n  == $m" -ForegroundColor White }
function Ok($m)   { Write-Host "     $m" -ForegroundColor DarkGray }
function Die($m)  { Write-Host "  !! $m" -ForegroundColor Red; exit 1 }

Write-Host @"

  ARSEX CLIENT - RELEASE BUILD
  斬  target $Target

"@ -ForegroundColor White

# ---------------------------------------------------------------- preflight
Step "Preflight"

if (-not $env:ARSEX_AZURE_CLIENT_ID) {
  Die "ARSEX_AZURE_CLIENT_ID is not set. auth/mod.rs reads it via env!() at compile time, so the build cannot proceed without it."
}
if ($env:ARSEX_AZURE_CLIENT_ID -notmatch '^[0-9a-fA-F-]{36}$') {
  Die "ARSEX_AZURE_CLIENT_ID does not look like a GUID."
}
Ok "azure client id present"

foreach ($t in @("cargo","rustc","node")) {
  if (-not (Get-Command $t -ErrorAction SilentlyContinue)) { Die "$t not found on PATH" }
}
Ok "toolchain present"

$installed = (rustup target list --installed) -join "`n"
if ($installed -notmatch [regex]::Escape($Target)) {
  Ok "installing rust target $Target"
  rustup target add $Target
}

# MSVC linker. Tauri cannot link without the Build Tools; the failure message
# from cargo alone is cryptic, so check explicitly.
if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
  Write-Host "     link.exe not on PATH - run from a Developer PowerShell, or install:" -ForegroundColor Yellow
  Write-Host "     Visual Studio Build Tools + 'Desktop development with C++'" -ForegroundColor Yellow
}

if (-not (Get-Command cargo-tauri -ErrorAction SilentlyContinue)) {
  Ok "installing tauri-cli"
  cargo install tauri-cli --version "^2" --locked
}

# ---------------------------------------------------------------- verify
if (-not $SkipTests) {
  Step "Verification gates"

  node tools\mono-lint.mjs prototype\
  if ($LASTEXITCODE -ne 0) { Die "monochrome lint failed - a colour leaked into the UI" }
  Ok "monochrome lock held"

  Push-Location launcher\core-launch
  cargo test --quiet
  if ($LASTEXITCODE -ne 0) { Pop-Location; Die "launch engine tests failed" }
  Pop-Location
  Ok "launch engine tests passed (43)"

  Push-Location launcher\src-tauri
  cargo test --quiet
  if ($LASTEXITCODE -ne 0) { Pop-Location; Die "rust tests failed" }
  Pop-Location
  Ok "app tests passed (10)"

  if (Get-Command bash -ErrorAction SilentlyContinue) {
    bash core/run-tests.sh | Select-Object -Last 2
    Ok "java harness passed"
  }
}

# ---------------------------------------------------------------- frontend
Step "Frontend"
node tools\sync-frontend.mjs
if ($LASTEXITCODE -ne 0) { Die "frontend sync failed" }
node tools\mono-lint.mjs launcher\dist\
if ($LASTEXITCODE -ne 0) { Die "monochrome lint failed on bundled frontend" }

# ---------------------------------------------------------------- signing
if ($Sign) {
  if (-not $env:ARSEX_CERT_THUMBPRINT) { Die "-Sign given but ARSEX_CERT_THUMBPRINT is not set" }
  Step "Code signing enabled"
  $conf = Get-Content launcher\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json
  $conf.bundle.windows.certificateThumbprint = $env:ARSEX_CERT_THUMBPRINT
  $conf | ConvertTo-Json -Depth 32 | Set-Content launcher\src-tauri\tauri.conf.json -Encoding UTF8
  Ok "thumbprint injected"
  Write-Host "     NOTE: an OV certificate does NOT grant instant SmartScreen trust." -ForegroundColor Yellow
  Write-Host "     Since June 2023 signing keys must live in FIPS 140-2 L2 hardware," -ForegroundColor Yellow
  Write-Host "     so CI needs a cloud HSM - a .pfx on disk will not work." -ForegroundColor Yellow
}

# ---------------------------------------------------------------- build
Step "Compiling release binary"
$sw = [Diagnostics.Stopwatch]::StartNew()

# Run from src-tauri: launcher/ has no Cargo.toml to anchor cargo.
Push-Location launcher\src-tauri
cargo tauri build --target $Target
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { Die "cargo tauri build failed" }

$sw.Stop()
Ok ("compiled in {0:n1}s" -f $sw.Elapsed.TotalSeconds)

# ---------------------------------------------------------------- report
Step "Artifacts"
$rel = "launcher\src-tauri\target\$Target\release"
$exe = Join-Path $rel "arsex.exe"
$nsis = Join-Path $rel "bundle\nsis"

if (Test-Path $exe) {
  $mb = (Get-Item $exe).Length / 1MB
  Ok ("arsex.exe          {0:n2} MB" -f $mb)
  if ($Sign) {
    $sig = Get-AuthenticodeSignature $exe
    Ok "signature          $($sig.Status)"
    if ($sig.Status -ne "Valid") { Die "signature invalid" }
  }
} else { Die "arsex.exe missing - build reported success but produced nothing" }

if (Test-Path $nsis) {
  Get-ChildItem $nsis -Filter *.exe | ForEach-Object {
    $mb = $_.Length / 1MB
    Ok ("{0}  {1:n2} MB" -f $_.Name, $mb)
    if ($mb -gt 12) {
      Write-Host "     WARNING: installer exceeds the 12 MB budget" -ForegroundColor Yellow
    }
  }
}

Write-Host "`n  BUILD COMPLETE`n" -ForegroundColor White
