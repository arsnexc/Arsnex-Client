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
    // Demo mode can never reach the JVM. Single chokepoint, cannot be routed around.
    if state.demo.load(std::sync::atomic::Ordering::Relaxed) && !auth::demo::can_launch() {
        return Err(
            "Demo mode cannot launch Minecraft. Sign in with a Microsoft account that owns the game."
                .into(),
        );
    }
    if let Some(s) = state.session.lock().unwrap().as_ref() {
        if s.is_running() {
            return Err("a game session is already running".into());
        }
    }

    // The official free demo: a REAL Microsoft session on an account without
    // entitlement, launched with Mojang's own --demo argument. The session is
    // resolved here in Rust so the token never crosses into the webview.
    // Owners and unknown ids pass straight through, unchanged.
    let mut demo = false;
    let (player, uuid, token) = match auth::resolve_launch_identity(&uuid).await {
        auth::LaunchIdentity::Demo(session) => {
            demo = true;
            tracing::info!(
                username = %session.username,
                "official demo: real Microsoft session, no Java entitlement"
            );
            (session.username, session.uuid, session.access_token.to_string())
        }
        auth::LaunchIdentity::Owner | auth::LaunchIdentity::Unknown => (player, uuid, token),
    };

    // The pipeline does blocking network and disk IO; keep it off the UI thread.
    let app2 = app.clone();
    let prepared = tauri::async_runtime::spawn_blocking(move || {
        game::pipeline::prepare(
            &app2, &instance, &version, &player, &uuid, &token, memory, java, demo,
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
    Ok(pid)
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
