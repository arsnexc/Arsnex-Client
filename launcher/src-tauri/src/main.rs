// No console window on a release build. Without this, double-clicking the exe
// flashes a black conhost window behind the launcher.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod game;
mod paths;

use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(Default)]
struct AppState {
    session: Mutex<Option<Arc<game::Session>>>,
    /// Set when the user chose demo mode. Gates the JVM spawn.
    demo: std::sync::atomic::AtomicBool,
    /// (instance slug, launch unix-second) of the running game, so play time
    /// is measured in Rust when the session ends — never client-claimed.
    play: Mutex<Option<(String, u64)>>,
    /// The explicit offline launch profile (settings → demo section).
    /// When set, LAUNCH uses this identity and no Microsoft session is
    /// attempted. In-memory only: never persisted as an account.
    offline: Mutex<Option<auth::offline::OfflineProfile>>,
}

#[tauri::command]
async fn launch_game(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    instance: String,
    version: String,
    player: String,
    uuid: String,
    token: String,
    memory: u32,
    java: Option<String>,
) -> Result<u32, String> {
    if let Some(s) = state.session.lock().unwrap().as_ref() {
        if s.is_running() {
            return Err("a game session is already running".into());
        }
    }

    // EVERY launch resolves a REAL Microsoft session here in Rust — owners
    // (full game) and demo-tier accounts alike — so a genuine token reaches
    // the JVM and nothing secret ever crosses into the webview. Until
    // v2.6.3 owners kept the empty string the webview passed: singleplayer
    // limped and servers rejected the join.
    // v2.12.0: an explicit OFFLINE PROFILE (settings → demo section) launches
    // with nothing but a username — the project owner's directed reversal of
    // the real-MSA-only rule, documented in README/ARSEX_SPEC. Offline goes
    // FIRST: it is the user's most explicit choice and needs no network.
    // Limits (also stated on the settings card): singleplayer + LAN yes,
    // online-mode servers no — there is no session token to validate.
    // Bind the clone first: a MutexGuard temporary inside the `if let`
    // scrutinee would live through the whole if/else (pre-2024-edition
    // temporary rule) and be held across the `.await` below — the guard is
    // !Send and Tauri command futures must be Send. Caught by the Windows
    // CI job (runs 60/61); the lib-only local check cannot see it.
    let offline_profile = state.offline.lock().unwrap().clone();
    let mut demo = false;
    let mut user_type = "msa";
    let (player, uuid, token) = if let Some(p) = offline_profile {
        tracing::info!(username = %p.name, "offline launch — no Microsoft session");
        user_type = "legacy";
        (p.name, p.uuid, String::new())
    } else if state.demo.load(std::sync::atomic::Ordering::Relaxed) && !auth::demo::can_launch() {
        // Demo mode can never reach the JVM. Single chokepoint, cannot be
        // routed around.
        return Err(
            "Demo mode cannot launch Minecraft. Sign in with a Microsoft account that owns the game."
                .into(),
        );
    } else {
        match auth::resolve_launch_identity(&uuid).await {
            Ok(auth::LaunchIdentity::Demo(session)) => {
                demo = true;
                tracing::info!(
                    username = %session.username,
                    "official demo: real Microsoft session, no Java entitlement"
                );
                (session.username, session.uuid, session.access_token.to_string())
            }
            Ok(auth::LaunchIdentity::Owner(session)) => {
                tracing::info!(
                    username = %session.username,
                    "owner launch: real Microsoft session resolved in Rust"
                );
                (session.username, session.uuid, session.access_token.to_string())
            }
            // No account matched: refuse rather than start an unauthenticated
            // session. The free demo tier is the account-less path.
            Ok(auth::LaunchIdentity::Unknown) => {
                return Err(
                    "no signed-in account for this launch — sign in with Microsoft first, \
                     or set an offline profile in Settings (username only, no servers)"
                        .into(),
                );
            }
            Err(e) => return Err(format!("launch sign-in failed: {e:#}")),
        }
    };

    // The pipeline does blocking network and disk IO; keep it off the UI thread.
    let app2 = app.clone();
    // Kept out of the closure: the play-time bookkeeping below needs the
    // slug after `instance` has been moved into spawn_blocking.
    let instance_slug = instance.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        game::pipeline::prepare(
            &app2, &instance, &version, &player, &uuid, &token, memory, java, demo, user_type,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // Log the exact command, with the token redacted.
    tracing::info!(
        java = %prepared.java.display(),
        args = %prepared.redacted.join(" "),
        "launching"
    );

    let session = game::launch(app, &prepared.java.to_string_lossy(), &prepared.argv, &prepared.cwd)
        .map_err(|e| e.to_string())?;
    let pid = session.pid;
    *state.session.lock().unwrap() = Some(session);
    // Play-time bookkeeping: the clock starts at handoff and is settled by
    // `note_session_end` when the game://exit event reaches the UI.
    *state.play.lock().unwrap() = Some((
        instance_slug,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    ));
    Ok(pid)
}

/// Set (validate) or clear the offline launch profile. `name: None` clears.
#[tauri::command]
fn set_offline_profile(
    state: tauri::State<'_, AppState>,
    name: Option<String>,
) -> Result<Option<auth::offline::OfflineProfile>, String> {
    let mut slot = state.offline.lock().unwrap();
    match name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => {
            let p = auth::offline::offline_profile(n)?;
            *slot = Some(p.clone());
            Ok(Some(p))
        }
        None => {
            *slot = None;
            Ok(None)
        }
    }
}

/// The current offline profile, or null.
#[tauri::command]
fn get_offline_profile(
    state: tauri::State<'_, AppState>,
) -> Option<auth::offline::OfflineProfile> {
    state.offline.lock().unwrap().clone()
}

/// Live session telemetry for the home panel.
#[derive(serde::Serialize)]
struct GameStats {
    pid: u32,
    instance: String,
    uptime_s: u64,
    /// Process working set, MB.
    memory_mb: u64,
    /// Process CPU percent (meaningful from the second poll on).
    cpu_pct: f32,
    /// Real frame rate from the mod's stats.json — present only when the
    /// report is fresh (< 10 s old) and the instance runs the Arsex mod.
    fps_avg: Option<u32>,
    fps_max: Option<u32>,
}

/// The running game's live numbers, or null when no session is live.
/// Memory/CPU come from the OS via sysinfo; FPS comes from the mod's
/// stats file, which vanilla instances never write — there the panel
/// honestly shows no FPS rather than a fabricated one.
#[tauri::command]
fn game_stats(state: tauri::State<'_, AppState>) -> Option<GameStats> {
    let session = state.session.lock().unwrap().clone()?;
    if !session.is_running() {
        return None;
    }
    let (slug, started) = state.play.lock().unwrap().clone()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut sys = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(session.pid);
    sys.refresh_process(pid);
    let (memory_mb, cpu_pct) = sys
        .process(pid)
        .map(|p| (p.memory() / 1024 / 1024, p.cpu_usage()))
        .unwrap_or((0, 0.0));

    // The mod reports fpsAvg/fpsMax/t (epoch ms). Fresh for 10 s.
    let (fps_avg, fps_max) = paths::instance_dir(&slug)
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("config").join("arsex").join("stats.json")).ok())
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            let t = v.get("t")?.as_u64()?;
            let fresh = now.saturating_mul(1000).saturating_sub(t) < 10_000;
            if !fresh {
                return None;
            }
            Some((
                v.get("fpsAvg").and_then(|x| x.as_u64()).map(|x| x as u32),
                v.get("fpsMax").and_then(|x| x.as_u64()).map(|x| x as u32),
            ))
        })
        .unwrap_or((None, None));

    Some(GameStats {
        pid: session.pid,
        instance: slug,
        uptime_s: now.saturating_sub(started),
        memory_mb,
        cpu_pct,
        fps_avg,
        fps_max,
    })
}

/// Settle the finished session: add the Rust-measured duration to the
/// instance's play_seconds and return the updated instance. No session or a
/// zero-duration session (instant crash) is a no-op, not an error.
#[tauri::command]
fn note_session_end(state: tauri::State<'_, AppState>) -> Result<Option<game::instance::Instance>, String> {
    let Some((slug, started)) = state.play.lock().unwrap().take() else {
        return Ok(None);
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs = now.saturating_sub(started) as u32;
    if secs == 0 {
        return Ok(None);
    }
    game::instance::add_play_seconds(&slug, secs)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// One release note for the news card.
#[derive(serde::Serialize)]
struct ReleaseNote {
    tag: String,
    /// ISO date, e.g. 2026-09-02.
    date: String,
    /// First ~200 characters of the release body, flattened to one line.
    excerpt: String,
    url: String,
}

/// The project's own releases, anonymously from the GitHub API — the news
/// card shows what actually shipped, never invented flavour text. Any
/// failure returns an empty list and the UI says OFFLINE.
#[tauri::command]
async fn latest_notes() -> Result<Vec<ReleaseNote>, String> {
    #[derive(serde::Deserialize)]
    struct GhRelease {
        tag_name: String,
        published_at: Option<String>,
        html_url: String,
        body: Option<String>,
    }
    let http = reqwest::Client::builder()
        .user_agent(concat!("ArsexClient/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let rels: Vec<GhRelease> = http
        .get("https://api.github.com/repos/arsnexc/Arsnex-Client/releases?per_page=3")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(rels
        .into_iter()
        .map(|r| ReleaseNote {
            tag: r.tag_name,
            date: r.published_at.unwrap_or_default().chars().take(10).collect(),
            excerpt: r
                .body
                .unwrap_or_default()
                .replace(['\r', '\n'], " ")
                .chars()
                .take(200)
                .collect(),
            url: r.html_url,
        })
        .collect())
}

/// Open an external link in the system browser — only exact-host https
/// links into the project's GitHub (see `paths::is_safe_external_url`).
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !paths::is_safe_external_url(&url) {
        return Err("refusing to open a link outside the project's GitHub".into());
    }
    open::that(url).map_err(|e| e.to_string())
}

#[tauri::command]
fn kill_game(state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(s) = state.session.lock().unwrap().as_ref() {
        s.kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn game_running(state: tauri::State<'_, AppState>) -> bool {
    state
        .session
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.is_running())
        .unwrap_or(false)
}

#[derive(serde::Serialize)]
struct ModScan {
    mods: Vec<arsex_launch::mods::ModInfo>,
    problems: Vec<arsex_launch::mods::ModProblem>,
    unreadable: Vec<(String, String)>,
}

#[tauri::command]
fn scan_mods(instance: String, loader: String) -> Result<ModScan, String> {
    use arsex_launch::mods::{scan_dir, validate, Loader};
    let dir = paths::instance_dir(&instance).map_err(|e| e.to_string())?.join("mods");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (mods, bad) = scan_dir(&dir);
    let target = match loader.to_lowercase().as_str() {
        "fabric" => Loader::Fabric,
        "quilt" => Loader::Quilt,
        "forge" => Loader::Forge,
        "neoforge" => Loader::NeoForge,
        _ => Loader::Unknown,
    };
    let problems = if target == Loader::Unknown { Vec::new() } else { validate(&mods, target) };
    Ok(ModScan {
        mods,
        problems,
        unreadable: bad.into_iter().map(|(p, e)| (p.display().to_string(), e)).collect(),
    })
}

/// Copy a jar into the instance and report what it actually is.
#[tauri::command]
fn install_mod(instance: String, source: String) -> Result<arsex_launch::mods::ModInfo, String> {
    let src = std::path::PathBuf::from(&source);
    // Validate BEFORE copying, so a bad file never lands in the mods folder.
    let info = arsex_launch::mods::read_mod(&src).map_err(|e| e.to_string())?;
    let dir = paths::instance_dir(&instance).map_err(|e| e.to_string())?.join("mods");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = src.file_name().ok_or("bad source path")?;
    let dest = dir.join(name);
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    let mut out = info;
    out.file = dest;
    Ok(out)
}

#[tauri::command]
fn toggle_mod(path: String, enabled: bool) -> Result<String, String> {
    arsex_launch::mods::set_enabled(std::path::Path::new(&path), enabled)
        .map(|p| p.display().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_mod(path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    // Refuse anything outside a managed mods directory.
    let cache = paths::cache_dir().map_err(|e| e.to_string())?;
    if !p.starts_with(&cache) {
        return Err("refusing to delete a file outside the Arsex data directory".into());
    }
    std::fs::remove_file(p).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_versions() -> Result<Vec<String>, String> {
    let c = reqwest::blocking::Client::builder()
        .user_agent(concat!("ArsexClient/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let m: arsex_launch::install::VersionManifest = c
        .get(arsex_launch::install::VERSION_MANIFEST)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    Ok(m.releases().into_iter().map(|v| v.id.clone()).collect())
}

#[tauri::command]
fn set_demo(state: tauri::State<'_, AppState>, on: bool) {
    state.demo.store(on, std::sync::atomic::Ordering::Relaxed);
}

// ------------------------------------------------------------------ instances

#[derive(serde::Deserialize)]
struct NewInstance {
    name: String,
    icon: u8,
    version: String,
    loader: String,
    /// Megabytes. The wizard slider is in GB and converts before sending.
    memory: u32,
    isolate_saves: bool,
    discord_rpc: bool,
    /// Slug of the instance whose config/ is cloned ("Copy current config").
    /// None when the option is off or no instance is active.
    copy_config_from: Option<String>,
    /// Performance mode: tuned JVM extras, fixed heap, above-normal priority.
    #[serde(default)]
    perf: bool,
}

/// Create an instance for real: directories, manifest, verified downloads.
/// Progress arrives on the frontend as `instance://stage` events.
#[tauri::command]
async fn create_instance(
    app: tauri::AppHandle,
    req: NewInstance,
) -> Result<game::instance::Instance, String> {
    let app2 = app.clone();
    // Network and disk IO; must not block the webview thread.
    tauri::async_runtime::spawn_blocking(move || {
        game::instance::create(
            &app2,
            game::instance::CreateRequest {
                name: req.name,
                icon: req.icon,
                version: req.version,
                loader: req.loader,
                memory: req.memory,
                isolate_saves: req.isolate_saves,
                discord_rpc: req.discord_rpc,
                copy_config_from: req.copy_config_from,
                perf: req.perf,
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn list_instances() -> Result<Vec<game::instance::Instance>, String> {
    game::instance::list().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_instance(slug: String) -> Result<(), String> {
    game::instance::remove(&slug).map_err(|e| e.to_string())
}

/// Toggle performance mode on an existing instance (MANAGE modal).
#[tauri::command]
fn set_instance_perf(slug: String, perf: bool) -> Result<game::instance::Instance, String> {
    game::instance::set_perf(&slug, perf).map_err(|e| e.to_string())
}

/// Right-size an instance's memory without recreating it.
#[tauri::command]
fn set_instance_memory(
    slug: String,
    memory: u32,
) -> Result<game::instance::Instance, String> {
    game::instance::set_memory(&slug, memory).map_err(|e| e.to_string())
}

/// Cheap pre-flight for the wizard's name field, so the user learns about a
/// collision while typing instead of after a multi-minute download.
#[tauri::command]
fn check_instance_name(name: String) -> Result<String, String> {
    let slug = game::instance::slugify(&name);
    if slug.is_empty() {
        return Err("Name must contain at least one letter or number.".into());
    }
    let taken = game::instance::list().map_err(|e| e.to_string())?;
    if taken.iter().any(|i| i.slug == slug) {
        return Err(format!("\u{201c}{}\u{201d} already exists.", name.trim()));
    }
    Ok(slug)
}

#[tauri::command]
fn open_log_dir() -> Result<(), String> {
    let d = paths::log_dir().map_err(|e| e.to_string())?;
    open::that(d).map_err(|e| e.to_string())
}

fn init_logging() -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let dir = paths::log_dir()?;
    let appender = tracing_appender::rolling::daily(&dir, "launcher.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_env("ARSEX_LOG").unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer().with_ansi(false).json().with_writer(writer))
        .init();
    Ok(guard)
}

fn main() {
    let _guard = init_logging().ok();

    // Any panic becomes a crash report on disk rather than a silent exit.
    std::panic::set_hook(Box::new(|info| {
        tracing::error!(target: "panic", "{info}");
        if let Ok(dir) = paths::crash_dir() {
            let f = dir.join(format!(
                "crash-{}.txt",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ));
            let _ = std::fs::write(f, format!("{info}\n\n{}", std::backtrace::Backtrace::force_capture()));
        }
    }));

    tauri::Builder::default()
        // Second launch focuses the existing window instead of starting a rival
        // instance that would fight over the same token vault.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            launch_game,
            kill_game,
            game_running,
            open_log_dir,
            set_demo,
            scan_mods,
            install_mod,
            toggle_mod,
            delete_mod,
            list_versions,
            create_instance,
            list_instances,
            delete_instance,
            set_instance_memory,
            set_instance_perf,
            note_session_end,
            game_stats,
            set_offline_profile,
            get_offline_profile,
            latest_notes,
            open_external,
            check_instance_name,
            auth::begin_demo,
            auth::begin_login,
            auth::current_account,
            auth::logout,
        ])
        .setup(|app| {
            tracing::info!("Arsex Client {} starting", env!("CARGO_PKG_VERSION"));
            let _ = app.handle().emit("app://ready", ());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start Arsex Client");
}
