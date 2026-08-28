//! Instance creation and registry.
//!
//! An instance is a directory under `%LOCALAPPDATA%\Arsex\instances\<slug>`
//! plus a record in `%APPDATA%\Arsex\instances.json`. Creation runs the same
//! download pipeline a launch does, so a freshly created instance is already
//! warm: nothing is downloaded twice on first play.

use anyhow::{anyhow, Context, Result};
use arsex_launch::install::{self, AssetIndex, VersionManifest};
use arsex_launch::manifest::Os;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub slug: String,
    pub name: String,
    /// Index into the wizard's icon list. Kept as an index, not a glyph, so
    /// the icon set can be re-skinned without migrating saved data.
    pub icon: u8,
    pub version: String,
    pub loader: String,
    pub memory: u32,
    pub isolate_saves: bool,
    pub discord_rpc: bool,
    /// Unix seconds. 0 means never launched.
    pub created: u64,
    pub last_played: u64,
}

#[derive(Clone, Serialize)]
struct Stage {
    key: String,
    label: String,
    pct: f32,
    detail: String,
}

fn stage(app: &AppHandle, key: &str, label: &str, pct: f32, detail: impl Into<String>) {
    let _ = app.emit(
        "instance://stage",
        Stage { key: key.into(), label: label.into(), pct, detail: detail.into() },
    );
}

/// Turn a display name into a filesystem-safe slug.
///
/// Collapses runs of non-alphanumerics to a single dash and trims them from
/// the ends, so "Ranked  BedWars!!" becomes "ranked-bedwars".
pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true; // leading dashes are suppressed
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.truncate(64);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn registry_path() -> Result<PathBuf> {
    Ok(crate::paths::data_dir()?.join("instances.json"))
}

pub fn list() -> Result<Vec<Instance>> {
    let p = registry_path()?;
    if !p.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read(&p)?;
    // A corrupt registry must not brick the launcher — start clean instead.
    Ok(serde_json::from_slice(&raw).unwrap_or_default())
}

fn write_registry(list: &[Instance]) -> Result<()> {
    let p = registry_path()?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    // Atomic swap: a crash mid-write cannot truncate the registry.
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

pub fn upsert(inst: Instance) -> Result<()> {
    let mut all = list()?;
    match all.iter_mut().find(|i| i.slug == inst.slug) {
        Some(existing) => *existing = inst,
        None => all.push(inst),
    }
    write_registry(&all)
}

pub fn remove(slug: &str) -> Result<()> {
    let mut all = list()?;
    all.retain(|i| i.slug != slug);
    write_registry(&all)?;
    // Registry first, then files: if deletion fails the entry is already gone
    // and the user is not stuck with an un-removable ghost row.
    if let Ok(dir) = crate::paths::instance_dir(slug) {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct CreateRequest {
    pub name: String,
    pub icon: u8,
    pub version: String,
    pub loader: String,
    pub memory: u32,
    pub isolate_saves: bool,
    pub discord_rpc: bool,
}

/// Create an instance for real: validate, make directories, fetch the version
/// manifest, download libraries and assets with SHA-1 verification, then
/// register it. Progress is reported through `instance://stage`.
pub fn create(app: &AppHandle, req: CreateRequest) -> Result<Instance> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("instance name cannot be empty"));
    }
    if name.chars().count() > 64 {
        return Err(anyhow!("instance name is too long (max 64 characters)"));
    }

    let slug = slugify(&name);
    if slug.is_empty() {
        return Err(anyhow!(
            "instance name must contain at least one letter or number"
        ));
    }
    if req.memory < 1024 || req.memory > 32768 {
        return Err(anyhow!("memory must be between 1 and 32 GB"));
    }

    let existing = list()?;
    if existing.iter().any(|i| i.slug == slug) {
        return Err(anyhow!("an instance named \"{name}\" already exists"));
    }

    stage(app, "dirs", "Creating instance", 3.0, &slug);
    let dir = crate::paths::instance_dir(&slug)
        .with_context(|| format!("creating instance directory for {slug}"))?;
    for sub in ["mods", "config", "resourcepacks", "shaderpacks", "screenshots"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    // Isolated saves get their own folder; shared saves symlink-free by design
    // (a junction would break when the shared instance is deleted).
    if req.isolate_saves {
        std::fs::create_dir_all(dir.join("saves"))?;
    }

    // From here on, a failure must not leave a half-built instance registered.
    let built = build(app, &slug, &req);
    if let Err(e) = built {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }

    let inst = Instance {
        slug,
        name,
        icon: req.icon,
        version: req.version,
        loader: req.loader,
        memory: req.memory,
        isolate_saves: req.isolate_saves,
        discord_rpc: req.discord_rpc,
        created: now(),
        last_played: 0,
    };
    upsert(inst.clone())?;
    stage(app, "done", "Instance ready", 100.0, &inst.name);
    Ok(inst)
}

/// The downloading half. Split out so `create` can clean up on any failure.
fn build(app: &AppHandle, slug: &str, req: &CreateRequest) -> Result<()> {
    let os = Os::current();
    let p = super::pipeline::Paths::for_instance(slug)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("ArsexClient/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let features: HashMap<String, bool> = HashMap::new();

    stage(app, "manifest", "Fetching version manifest", 8.0, "launchermeta.mojang.com");
    let manifest: VersionManifest = client
        .get(install::VERSION_MANIFEST)
        .send()?
        .error_for_status()?
        .json()
        .context("parsing version manifest")?;

    stage(app, "version", "Resolving version", 14.0, &req.version);
    let version = super::pipeline::resolve_version(&client, &manifest, &req.version, &p.versions)
        .with_context(|| format!("resolving Minecraft {}", req.version))?;

    stage(
        app,
        "libraries",
        "Downloading libraries",
        20.0,
        format!("{} libraries declared", version.libraries.len()),
    );
    let tasks = install::plan_downloads(&version, &p.libraries, &p.versions, os, &features);
    download(app, &client, &tasks, "libraries", "Downloading libraries", 20.0, 40.0)?;

    stage(app, "assets", "Verifying assets", 62.0, "reading asset index");
    if let Some(ai) = &version.asset_index {
        let idx = p.assets.join("indexes").join(format!("{}.json", ai.id));
        let raw = if install::verified(&idx, &ai.sha1) {
            std::fs::read(&idx)?
        } else {
            let b = client.get(&ai.url).send()?.error_for_status()?.bytes()?.to_vec();
            std::fs::create_dir_all(idx.parent().unwrap())?;
            std::fs::write(&idx, &b)?;
            b
        };
        let index: AssetIndex = serde_json::from_slice(&raw)?;
        let tasks = index.plan(&p.assets);
        download(app, &client, &tasks, "assets", "Downloading assets", 62.0, 33.0)?;
    }

    // The loader jar itself is not fetched here: Fabric/Forge profiles are
    // installed as a separate version id, and the wizard records the choice so
    // the first launch resolves it. Vanilla needs nothing further.
    if !req.loader.eq_ignore_ascii_case("vanilla") {
        stage(
            app,
            "loader",
            format!("{} selected", req.loader).as_str(),
            97.0,
            "loader profile resolves on first launch",
        );
    }
    Ok(())
}

fn download(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    tasks: &[install::DownloadTask],
    key: &str,
    label: &str,
    base: f32,
    span: f32,
) -> Result<()> {
    let total = install::total_bytes(tasks).max(1);
    let mut done: u64 = 0;
    for (i, t) in tasks.iter().enumerate() {
        install::fetch(t, client).with_context(|| format!("downloading {}", t.url))?;
        done += t.size;
        // Same throttle as the launch pipeline: 4000 asset files would
        // otherwise emit 4000 IPC messages and stall the webview.
        if i % 25 == 0 || i + 1 == tasks.len() {
            let frac = done as f32 / total as f32;
            stage(
                app,
                key,
                label,
                base + span * frac,
                format!("{}/{} files · {:.1} MB", i + 1, tasks.len(), done as f64 / 1e6),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("Ranked BedWars"), "ranked-bedwars");
        assert_eq!(slugify("main"), "main");
        assert_eq!(slugify("  Shadow  "), "shadow");
    }

    #[test]
    fn slugify_collapses_and_trims() {
        assert_eq!(slugify("Ranked   BedWars!!"), "ranked-bedwars");
        assert_eq!(slugify("--hello--"), "hello");
        assert_eq!(slugify("a...b"), "a-b");
    }

    #[test]
    fn slugify_rejects_traversal_shapes() {
        // The characters that make traversal possible cannot survive slugify.
        assert_eq!(slugify(".."), "");
        assert_eq!(slugify("../../windows"), "windows");
        assert_eq!(slugify("a\\b"), "a-b");
        assert_eq!(slugify("a/b"), "a-b");
    }

    #[test]
    fn slugify_handles_unicode_and_empty() {
        // Non-ASCII is dropped rather than transliterated; a name of only
        // non-ASCII yields an empty slug, which create() rejects with a message.
        assert_eq!(slugify("斬"), "");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("   "), "");
    }

    #[test]
    fn slugify_is_length_capped() {
        let s = slugify(&"a".repeat(200));
        assert_eq!(s.len(), 64);
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn slugify_output_is_always_a_valid_slug() {
        for name in ["Ranked BedWars", "a/b", "..", "斬 katana", "x".repeat(99).as_str()] {
            let s = slugify(name);
            assert!(
                s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "bad slug from {name:?}: {s:?}"
            );
            assert!(!s.starts_with('-') && !s.ends_with('-'));
        }
    }

    #[test]
    fn instance_roundtrips_through_json() {
        let i = Instance {
            slug: "main".into(),
            name: "Main".into(),
            icon: 2,
            version: "1.20.4".into(),
            loader: "Fabric".into(),
            memory: 4096,
            isolate_saves: true,
            discord_rpc: false,
            created: 1700000000,
            last_played: 0,
        };
        let raw = serde_json::to_vec(&i).unwrap();
        let back: Instance = serde_json::from_slice(&raw).unwrap();
        assert_eq!(i, back);
    }
}
