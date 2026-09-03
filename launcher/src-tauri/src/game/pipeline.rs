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
        // A dead route must fail in seconds, not hang the pipeline until the
        // 120 s total timeout fires — that hang is what "stuck at 6%" was.
        .connect_timeout(std::time::Duration::from_secs(15))
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
    // Six parallel workers with per-file retry (see install::fetch_all).
    // Progress comes from the engine, throttled to every 25 files, so the
    // IPC bridge never floods — and the fraction is byte-based now, which
    // reads truer on asset passes where file sizes vary 200 B..1 MB.
    install::fetch_all(client, tasks, &|files, of, bytes, _tb| {
        stage(
            app,
            key,
            label,
            base_pct + span_pct * (bytes as f32 / total as f32),
            format!("{files}/{of} files · {:.1} MB", bytes as f64 / 1e6),
        );
    })
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
    demo: bool,
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

    // A FABRIC instance is launched through a loader profile layered on the
    // vanilla version, not the vanilla id itself. Before this block, choosing
    // FABRIC in the wizard stored a string that resolve_version could not
    // resolve, and the game silently started without any mods loaded.
    let inst = crate::game::instance::list()
        .ok()
        .and_then(|l| l.into_iter().find(|i| i.slug == instance));
    let is_fabric = inst
        .as_ref()
        .map(|i| i.loader.eq_ignore_ascii_case("fabric"))
        .unwrap_or(false);
    let perf = inst.as_ref().map(|i| i.perf).unwrap_or(false);
    let version_id = if is_fabric {
        let mc = inst
            .as_ref()
            .map(|i| i.version.clone())
            .unwrap_or_else(|| version_id.to_string());
        provision_fabric(app, &client, &p, &mc, 6.0)?
    } else {
        version_id.to_string()
    };

    stage(app, "version", "Resolving version", 10.0, &version_id);
    let version = resolve_version(&client, &manifest, &version_id, &p.versions)?;

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
        // Warm launches skip the ~500 MB re-hash: asset storage is
        // content-addressed, so existence+size is sound while the index
        // JSON itself stays SHA-1 verified above. The stamp is rewritten
        // after every fully successful pass.
        let stamp = AssetIndex::stamp_path(&p.assets, &ai.id);
        let fast = stamp.exists();
        if fast {
            stage(app, "assets", "Verifying assets", 52.0, "warm pass · sizes only");
        }
        let tasks = index.plan(&p.assets, fast);
        run_downloads(app, &client, &tasks, "assets", "Downloading assets", 52.0, 26.0)?;
        std::fs::write(&stamp, b"verified\n")?;
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
        // Performance mode pairs the extra JVM flags with a FIXED heap:
        // min == max removes the resize collections that stutter mid-game.
        min_memory: if perf { max_mem } else { (max_mem / 4).max(512) },
        demo,
        perf,
    };

    let argv = args::build(&version, &ctx, os);
    let redacted = args::redact(&argv, token);

    let java_bin = java.unwrap_or_else(|| default_java(os));
    // Preflight the JVM BEFORE handing off: a missing or too-old Java must
    // fail with words, not with a process error nobody reads. This is the
    // most common failure on a fresh Windows machine with no JDK installed.
    preflight_java(&java_bin, version.required_java())?;

    stage(app, "ready", "Starting JVM", 100.0, format!("{} arguments", argv.len()));

    Ok(Prepared {
        java: PathBuf::from(java_bin),
        argv,
        cwd: p.instance,
        redacted,
        required_java: version.required_java(),
    })
}

/// Read a `java -version` banner and return its MAJOR version:
/// `17.0.2` -> 17, `1.8.0_391` -> 8 (legacy scheme), `21.0.5+11` -> 21.
pub fn parse_java_major(banner: &str) -> Option<u32> {
    // The version is the first quoted run of the banner.
    let q = banner.split('"').nth(1)?;
    let head = q.split(['.', '_', '+', '-']).next()?;
    if head == "1" {
        // Legacy 1.8.0_x scheme: the minor IS the major. No JDK ever shipped
        // 1.9+ under this scheme (versioning jumped to 9), so a "1.21" banner
        // is not a JDK we understand — refuse instead of guessing 21.
        let minor: u32 = q.split('.').nth(1)?.split(['_', '+', '-']).next()?.parse().ok()?;
        return (minor <= 8).then_some(minor);
    }
    head.parse().ok()
}

/// Verify the launch Java exists and is new enough — with words.
pub fn preflight_java(java: &str, required: u32) -> Result<()> {
    use std::process::Command;
    let out = match Command::new(java).arg("-version").output() {
        Ok(o) => o,
        Err(_) => anyhow::bail!(
            "Java was not found ({java}). This instance needs Java {required} or newer. \
             Install Temurin {required}+ from https://adoptium.net and relaunch."
        ),
    };
    let banner = String::from_utf8_lossy(&out.stderr);
    let Some(found) = parse_java_major(&banner) else {
        anyhow::bail!(
            "could not read a Java version from `{java} -version` — is it a real JDK? \
             This instance needs Java {required}+."
        );
    };
    if found < required {
        anyhow::bail!(
            "Java {found} is too old for this instance — Minecraft needs Java {required}+. \
             Install Temurin {required}+ from https://adoptium.net and relaunch."
        );
    }
    Ok(())
}

fn default_java(os: Os) -> String {
    match os {
        // javaw has no console window; java would spawn a stray conhost.
        Os::Windows => "javaw".into(),
        _ => "java".into(),
    }
}

/// Install the full Fabric stack into an instance: loader profile, fabric-api,
/// and the embedded Arsex mod. Returns the version id to launch with.
///
/// Everything here is idempotent — a warm instance re-verifies in milliseconds
/// and downloads nothing.
/// `base_pct` positions the three loader stages on the caller's progress
/// scale: the launch pipeline passes 6.0 (before "Resolving version" at 10),
/// instance creation passes 96.0 (after assets, before "done" at 100).
pub(crate) fn provision_fabric(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    p: &Paths,
    mc_version: &str,
    base_pct: f32,
) -> Result<String> {
    use arsex_launch::fabric;

    stage(
        app,
        "loader",
        "Installing Fabric loader",
        base_pct,
        format!("fabric-loader {}", fabric::LOADER_VERSION),
    );
    let profile_id = fabric::ensure_loader_profile(client, mc_version, &p.versions)
        .context("setting up the Fabric loader")?;

    // fabric-api and the Arsex mod are pinned to MOD_TARGET_MC. On any other
    // version they are skipped with an explicit event, NOT installed into a
    // game that would crash on them at mod resolution.
    if !fabric::mod_stack_supported(mc_version) {
        let _ = app.emit(
            "launch://mod-problem",
            mods::ModProblem {
                kind: "version_mismatch".into(),
                mod_id: "arsex".into(),
                detail: format!(
                    "Arsex modules target Minecraft {} — this instance runs {}, \
                     so the loader starts without them",
                    fabric::MOD_TARGET_MC,
                    mc_version
                ),
            },
        );
        return Ok(profile_id);
    }

    stage(
        app,
        "loader",
        "Verifying fabric-api",
        base_pct + 0.5,
        fabric::FABRIC_API_VERSION,
    );
    let api_path = fabric::ensure_fabric_api(client, &p.root)
        .context("verifying fabric-api (pinned SHA-256)")?;
    install_respecting_disable(
        &std::fs::read(&api_path).context("reading the cached fabric-api jar")?,
        &p.instance.join("mods"),
        &api_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "fabric-api.jar".into()),
    )?;

    // The Arsex mod itself, embedded in the binary at compile time.
    // First: evict any OLDER embedded version. The version lives in the file
    // name, so without this a 2.5.0 jar would sit next to the new one and
    // Fabric would load both — duplicate modules, duplicate keybinds.
    let current_mod = crate::game::bundled::jar_file_name();
    prune_old_mod_jars(&p.instance.join("mods"), &current_mod);

    match crate::game::bundled::jar_bytes() {
        Some(bytes) => {
            stage(app, "loader", "Installing Arsex modules", base_pct + 1.0, crate::game::bundled::ARSEX_MOD_VERSION);
            install_respecting_disable(
                bytes,
                &p.instance.join("mods"),
                &crate::game::bundled::jar_file_name(),
            )?;
        }
        None => {
            // A dev build without the jar must say so, not pretend.
            let _ = app.emit(
                "launch://mod-problem",
                mods::ModProblem {
                    kind: "not_bundled".into(),
                    mod_id: "arsex".into(),
                    detail: "this launcher build embeds no Arsex mod jar; the \
                             game will start without Arsex modules"
                        .into(),
                },
            );
        }
    }
    Ok(profile_id)
}

/// Remove `arsex-mod-*.jar` (and its .disabled form) files that are not the
/// current embedded version. User-added mods are never touched; only files
/// the launcher itself named.
fn prune_old_mod_jars(mods_dir: &Path, keep: &str) {
    let Ok(rd) = std::fs::read_dir(mods_dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let stale = name.starts_with("arsex-mod-")
            && (name.ends_with(".jar") || name.ends_with(".jar.disabled"))
            && name != keep
            && name != format!("{keep}.disabled");
        if stale {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// install_mod_jar, except a jar the user deliberately disabled (the portable
/// `name.jar.disabled` convention) stays disabled: re-provisioning must never
/// undo an explicit choice just because the user pressed LAUNCH again.
fn install_respecting_disable(bytes: &[u8], mods_dir: &Path, file_name: &str) -> Result<()> {
    let dest = mods_dir.join(file_name);
    if !dest.exists() && mods_dir.join(format!("{file_name}.disabled")).exists() {
        return Ok(());
    }
    fabric_install(bytes, mods_dir, file_name)
}

// Thin indirection so the call site reads clearly and the test below can hit
// the policy without touching the network.
fn fabric_install(bytes: &[u8], mods_dir: &Path, file_name: &str) -> Result<()> {
    arsex_launch::fabric::install_mod_jar(bytes, mods_dir, file_name).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_embedded_mod_jars_are_pruned_but_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("arsex-mod-2.5.0.jar"), b"old").unwrap();
        std::fs::write(mods.join("arsex-mod-2.5.0.jar.disabled"), b"old").unwrap();
        std::fs::write(mods.join("fabric-api-0.97.0+1.20.4.jar"), b"api").unwrap();
        std::fs::write(mods.join("sodium.jar"), b"user mod").unwrap();
        super::prune_old_mod_jars(&mods, "arsex-mod-2.6.0.jar");
        assert!(!mods.join("arsex-mod-2.5.0.jar").exists(), "old jar evicted");
        assert!(!mods.join("arsex-mod-2.5.0.jar.disabled").exists(), "old disabled evicted");
        assert!(mods.join("fabric-api-0.97.0+1.20.4.jar").exists(), "fabric-api untouched");
        assert!(mods.join("sodium.jar").exists(), "user mods untouched");
    }

    #[test]
    fn java_banner_parses_across_schemes() {
        assert_eq!(parse_java_major("openjdk version \"17.0.20.1\" 2026-08-18"), Some(17));
        assert_eq!(parse_java_major("java version \"1.8.0_391\""), Some(8));
        assert_eq!(parse_java_major("openjdk version \"21.0.5\" 2024-10-15 LTS"), Some(21));
        assert_eq!(parse_java_major("openjdk version \"1.21.0.3\""), None, "weird 1.x is not guessed");
        assert_eq!(parse_java_major(""), None);
    }

    #[test]
    fn missing_java_is_refused_with_guidance() {
        let e = preflight_java("/nonexistent-java-binary", 17).unwrap_err();
        let m = format!("{e:#}");
        assert!(m.contains("Java was not found"), "{m}");
        assert!(m.contains("adoptium.net"), "must point at the fix: {m}");
        assert!(m.contains("17"), "must name the required major: {m}");
    }

    #[test]
    fn a_disabled_mod_is_not_resurrected() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("arsex-mod-2.5.0.jar.disabled"), b"old bytes").unwrap();

        install_respecting_disable(b"new bytes", &mods, "arsex-mod-2.5.0.jar").unwrap();
        assert!(!mods.join("arsex-mod-2.5.0.jar").exists(), "disabled choice must hold");
        assert_eq!(
            std::fs::read(mods.join("arsex-mod-2.5.0.jar.disabled")).unwrap(),
            b"old bytes"
        );
    }

    #[test]
    fn an_enabled_or_missing_mod_provisions_normally() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        // Absent -> installed.
        install_respecting_disable(b"v1", &mods, "a.jar").unwrap();
        assert_eq!(std::fs::read(mods.join("a.jar")).unwrap(), b"v1");
        // Present but stale -> upgraded.
        install_respecting_disable(b"v2", &mods, "a.jar").unwrap();
        assert_eq!(std::fs::read(mods.join("a.jar")).unwrap(), b"v2");
    }
}
