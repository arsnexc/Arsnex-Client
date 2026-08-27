//! Launch orchestration: manifest -> downloads -> natives -> classpath -> spawn.
//!
//! Emits `launch://stage` events so the UI shows real progress against real
//! byte counts, not a scripted animation.

use anyhow::{anyhow, Context, Result};
use arsex_launch::args::{self, LaunchContext};
use arsex_launch::install::{self, AssetIndex, DownloadTask, VersionManifest};
use arsex_launch::manifest::{Os, VersionJson};
use arsex_launch::mods;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
pub struct Stage {
    pub key: String,
    pub label: String,
    pub pct: f32,
    pub detail: String,
}

fn stage(app: &AppHandle, key: &str, label: &str, pct: f32, detail: impl Into<String>) {
    let _ = app.emit(
        "launch://stage",
        Stage { key: key.into(), label: label.into(), pct, detail: detail.into() },
    );
}

pub struct Paths {
    pub root: PathBuf,
    pub libraries: PathBuf,
    pub assets: PathBuf,
    pub versions: PathBuf,
    pub instance: PathBuf,
    pub natives: PathBuf,
}

impl Paths {
    pub fn for_instance(slug: &str) -> Result<Paths> {
        let cache = crate::paths::cache_dir()?;
        let instance = crate::paths::instance_dir(slug)?;
        Ok(Paths {
            libraries: cache.join("libraries"),
            assets: cache.join("assets"),
            versions: cache.join("versions"),
            natives: instance.join("natives"),
            instance,
            root: cache,
        })
    }
}

fn http() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("ArsexClient/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?)
}

/// Resolve a version id, following `inheritsFrom` for modloader profiles.
pub fn resolve_version(
    client: &reqwest::blocking::Client,
    manifest: &VersionManifest,
    id: &str,
    versions_dir: &Path,
) -> Result<VersionJson> {
    // A modloader profile lives on disk as versions/<id>/<id>.json.
    let local = versions_dir.join(id).join(format!("{id}.json"));
    let v: VersionJson = if local.exists() {
        serde_json::from_slice(&std::fs::read(&local)?)
            .with_context(|| format!("parsing {}", local.display()))?
    } else {
        let entry = manifest
            .find(id)
            .ok_or_else(|| anyhow!("unknown Minecraft version '{id}'"))?;
        let raw = client.get(&entry.url).send()?.error_for_status()?.bytes()?;
        std::fs::create_dir_all(local.parent().unwrap())?;
        std::fs::write(&local, &raw)?;
        serde_json::from_slice(&raw)?
    };

    if let Some(parent_id) = v.inherits_from.clone() {
        let parent = resolve_version(client, manifest, &parent_id, versions_dir)?;
        return Ok(VersionJson::inherit(v, parent));
    }
    Ok(v)
}

fn run_downloads(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    tasks: &[DownloadTask],
    key: &str,
    label: &str,
    base_pct: f32,
    span_pct: f32,
) -> Result<()> {
    let total = install::total_bytes(tasks).max(1);
    let mut done: u64 = 0;
    for (i, t) in tasks.iter().enumerate() {
        install::fetch(t, client)
            .with_context(|| format!("downloading {}", t.url))?;
        done += t.size;
        // Throttle events; 4000 asset files would otherwise flood the IPC bridge.
        if i % 25 == 0 || i + 1 == tasks.len() {
            let frac = done as f32 / total as f32;
            stage(
                app,
                key,
                label,
                base_pct + span_pct * frac,
                format!("{}/{} files · {:.1} MB", i + 1, tasks.len(), done as f64 / 1e6),
            );
        }
    }
    Ok(())
}

pub struct Prepared {
    pub java: PathBuf,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub redacted: Vec<String>,
    pub required_java: u32,
}

/// Everything up to (not including) spawning the JVM.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    app: &AppHandle,
    instance: &str,
    version_id: &str,
    player: &str,
    uuid: &str,
    token: &str,
    max_mem: u32,
    java: Option<String>,
) -> Result<Prepared> {
    let os = Os::current();
    let p = Paths::for_instance(instance)?;
    let client = http()?;
    let features: HashMap<String, bool> = HashMap::new();

    stage(app, "manifest", "Fetching version manifest", 4.0, "launchermeta.mojang.com");
    let manifest: VersionManifest = client
        .get(install::VERSION_MANIFEST)
        .send()?
        .error_for_status()?
        .json()?;

    stage(app, "version", "Resolving version", 10.0, version_id);
    let version = resolve_version(&client, &manifest, version_id, &p.versions)?;

    stage(
        app,
        "libraries",
        "Downloading libraries",
        16.0,
        format!("{} libraries declared", version.libraries.len()),
    );
    let lib_tasks = install::plan_downloads(&version, &p.libraries, &p.versions, os, &features);
    run_downloads(app, &client, &lib_tasks, "libraries", "Downloading libraries", 16.0, 34.0)?;

    stage(app, "assets", "Verifying assets", 52.0, "reading asset index");
    if let Some(ai) = &version.asset_index {
        let idx_path = p.assets.join("indexes").join(format!("{}.json", ai.id));
        let raw = if install::verified(&idx_path, &ai.sha1) {
            std::fs::read(&idx_path)?
        } else {
            let b = client.get(&ai.url).send()?.error_for_status()?.bytes()?.to_vec();
            std::fs::create_dir_all(idx_path.parent().unwrap())?;
            std::fs::write(&idx_path, &b)?;
            b
        };
        let index: AssetIndex = serde_json::from_slice(&raw)?;
        let tasks = index.plan(&p.assets);
        run_downloads(app, &client, &tasks, "assets", "Downloading assets", 52.0, 26.0)?;
    }

    stage(app, "natives", "Extracting natives", 80.0, "");
    let _ = std::fs::remove_dir_all(&p.natives); // stale natives break startup
    let mut extracted = 0usize;
    for lib in &version.libraries {
        if !lib.applies(os, &features) {
            continue;
        }
        if let Some(key) = lib.natives_key(os) {
            if let Some(a) = lib.downloads.classifiers.get(&key) {
                let rel = a.path.clone().unwrap_or_default();
                let jar = p.libraries.join(&rel);
                if jar.exists() {
                    let exclude = lib
                        .extract
                        .as_ref()
                        .map(|e| e.exclude.clone())
                        .unwrap_or_else(|| vec!["META-INF/".to_string()]);
                    extracted += install::extract_natives(&jar, &p.natives, &exclude)?;
                }
            }
        }
    }
    stage(app, "natives", "Extracting natives", 84.0, format!("{extracted} files"));

    // Report mod problems before launching rather than after a crash.
    let mods_dir = p.instance.join("mods");
    if mods_dir.exists() {
        let (found, bad) = mods::scan_dir(&mods_dir);
        let target = if version.main_class.contains("fabric") {
            mods::Loader::Fabric
        } else if version.main_class.contains("forge") || version.main_class.contains("bootstrap") {
            mods::Loader::Forge
        } else {
            mods::Loader::Unknown
        };
        if target != mods::Loader::Unknown {
            for prob in mods::validate(&found, target) {
                let _ = app.emit("launch://mod-problem", &prob);
            }
        }
        for (path, err) in bad {
            let _ = app.emit(
                "launch://mod-problem",
                mods::ModProblem {
                    kind: "unreadable".into(),
                    mod_id: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                    detail: err,
                },
            );
        }
        stage(app, "mods", "Checking mods", 88.0, format!("{} mods", found.len()));
    }

    stage(app, "classpath", "Building classpath", 92.0, "");
    let cp = install::build_classpath(&version, &p.libraries, &p.versions, os, &features);
    let classpath = args::join_classpath(&cp, os);

    let ctx = LaunchContext {
        player_name: player.to_string(),
        uuid: uuid.to_string(),
        access_token: token.to_string(),
        user_type: "msa".into(),
        version_id: version.id.clone(),
        version_type: "release".into(),
        game_dir: args::path_str(&p.instance),
        assets_dir: args::path_str(&p.assets),
        assets_index: version
            .asset_index
            .as_ref()
            .map(|a| a.id.clone())
            .or_else(|| version.assets.clone())
            .unwrap_or_else(|| "legacy".into()),
        natives_dir: args::path_str(&p.natives),
        classpath,
        launcher_name: "arsex".into(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        width: None,
        height: None,
        max_memory: max_mem,
        min_memory: (max_mem / 4).max(512),
    };

    let argv = args::build(&version, &ctx, os);
    let redacted = args::redact(&argv, token);

    stage(app, "ready", "Starting JVM", 100.0, format!("{} arguments", argv.len()));

    Ok(Prepared {
        java: PathBuf::from(java.unwrap_or_else(|| default_java(os))),
        argv,
        cwd: p.instance,
        redacted,
        required_java: version.required_java(),
    })
}

fn default_java(os: Os) -> String {
    match os {
        // javaw has no console window; java would spawn a stray conhost.
        Os::Windows => "javaw".into(),
        _ => "java".into(),
    }
}
