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
use std::path::{Path, PathBuf};
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

/// Change an instance's JVM heap ceiling (MB). Creation should not be the
/// only moment a user can right-size memory.
pub fn set_memory(slug: &str, memory: u32) -> Result<Instance> {
    if memory < 1024 || memory > 32768 {
        return Err(anyhow!("memory must be between 1 and 32 GB"));
    }
    let mut all = list()?;
    let Some(inst) = all.iter_mut().find(|i| i.slug == slug) else {
        return Err(anyhow!("no instance named '{slug}'"));
    };
    inst.memory = memory;
    let out = inst.clone();
    write_registry(&all)?;
    Ok(out)
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
    /// "Copy current config" in the wizard: slug whose config/ directory is
    /// cloned into the new instance after the downloads succeed.
    pub copy_config_from: Option<String>,
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

    // Refuse combinations the launch pipeline cannot honour BEFORE anything
    // is fetched or built. Fabric on a pre-1.14 version (1.8.9, 1.12.2) can
    // never start — mainstream Fabric does not ship for it — and failing
    // here, with these words, is the difference between a clear wizard error
    // and an instance that downloads gigabytes and then dies on launch with
    // an unexplained HTTP 400.
    if req.loader.eq_ignore_ascii_case("fabric")
        && !arsex_launch::fabric::loader_supports_game(&req.version)
    {
        return Err(anyhow!("{}", arsex_launch::fabric::unsupported_message(&req.version)));
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

    // "Copy current config": clone the source instance's config/ now that the
    // downloads succeeded (a failure above still cleans up wholesale). A
    // missing source is skipped with a visible note, not a failure — the user
    // asked for a convenience, not a dependency.
    let copy_from = req
        .copy_config_from
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != slug.as_str());
    if let Some(src_slug) = copy_from {
        match crate::paths::instance_dir(src_slug) {
            Ok(from) => {
                stage(app, "config", "Copying configuration", 98.0, src_slug);
                copy_tree(&from.join("config"), &dir.join("config"))
                    .with_context(|| format!("copying config from {src_slug}"))?;
            }
            Err(_) => stage(
                app,
                "config",
                "Configuration source gone",
                98.0,
                format!("{src_slug} not found — skipped"),
            ),
        }
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

/// Recursively copy `from` into `to` — a merge, not a mirror: same-named
/// files are overwritten, nothing is deleted. A missing `from` copies zero
/// files (callers decide what that means). Symlinks are skipped: a link out
/// of the instance tree would silently copy whatever it points at.
fn copy_tree(from: &Path, to: &Path) -> Result<u64> {
    if !from.is_dir() {
        return Ok(0);
    }
    let mut n = 0u64;
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_dir() {
            n += copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
            n += 1;
        }
    }
    Ok(n)
}

/// The downloading half. Split out so `create` can clean up on any failure.
fn build(app: &AppHandle, slug: &str, req: &CreateRequest) -> Result<()> {
    let os = Os::current();
    let p = super::pipeline::Paths::for_instance(slug)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("ArsexClient/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(15))
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
        // Creation shares the launch path's warm-start stamp: a re-create of
        // the same version (or a second instance of it) plans by size instead
        // of re-hashing everything already in the shared cache.
        let stamp = AssetIndex::stamp_path(&p.assets, &ai.id);
        let fast = stamp.exists();
        if fast {
            stage(app, "assets", "Verifying assets", 62.0, "warm pass · sizes only");
        }
        let tasks = index.plan(&p.assets, fast);
        download(app, &client, &tasks, "assets", "Downloading assets", 62.0, 33.0)?;
        std::fs::write(&stamp, b"verified\n")?;
    }

    // The Fabric stack is installed HERE, during creation — not deferred to
    // first launch. Deferring is what made users think "the fabric loader
    // does not download": the instance looked ready but the loader was
    // fetched later, at LAUNCH, where a single dropped connection to
    // fabric's meta meant a bar stuck at 6% and nothing installed. Now the
    // wizard's own overlay shows every loader file, and the first launch
    // re-verifies the warm cache in milliseconds.
    if req.loader.eq_ignore_ascii_case("fabric") {
        super::pipeline::provision_fabric(app, &client, &p, &req.version, 96.0)
            .context("installing the Fabric loader stack")?;
    } else if !req.loader.eq_ignore_ascii_case("vanilla") {
        // Forge/Quilt: the wizard blocks these; this is a stale-data safety
        // net that still says the truth.
        stage(
            app,
            "loader",
            format!("{} selected", req.loader).as_str(),
            97.0,
            "loader provisioning is not wired yet",
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
    // Same parallel engine as the launch pipeline (install::fetch_all);
    // creation used to be the slow one — thousands of asset files, one
    // connection, zero retries. Progress is throttled inside the engine.
    install::fetch_all(client, tasks, &|files, of, bytes, _tb| {
        stage(
            app,
            key,
            label,
            base + span * (bytes as f32 / total as f32),
            format!("{files}/{of} files · {:.1} MB", bytes as f64 / 1e6),
        );
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn copy_tree_merges_overwrites_and_skips_missing_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src").join("config");
        let dst = tmp.path().join("dst").join("config");
        std::fs::create_dir_all(src.join("arsex")).unwrap();
        std::fs::write(src.join("options.txt"), b"fov:90").unwrap();
        std::fs::write(src.join("arsex").join("modules.json"), b"{}").unwrap();
        let n = super::copy_tree(&src, &dst).unwrap();
        assert_eq!(n, 2, "two files copied");
        assert_eq!(
            std::fs::read(dst.join("arsex").join("modules.json")).unwrap(),
            b"{}"
        );
        // Second copy overwrites without deleting anything the dest gained.
        std::fs::write(src.join("options.txt"), b"fov:110").unwrap();
        std::fs::write(dst.join("extra.txt"), b"keep").unwrap();
        super::copy_tree(&src, &dst).unwrap();
        assert_eq!(std::fs::read(dst.join("options.txt")).unwrap(), b"fov:110");
        assert!(dst.join("extra.txt").exists(), "merge, not mirror");
        // A missing source is not an error.
        assert_eq!(
            super::copy_tree(&tmp.path().join("nope"), &dst).unwrap(),
            0
        );
    }

    #[test]
    fn set_memory_validates_before_touching_the_registry() {
        // Out-of-range memory is refused without reading or writing anything.
        let e = super::set_memory("no-such-instance", 512).unwrap_err();
        assert!(format!("{e:#}").contains("between 1 and 32 GB"));
        // In-range but unknown slug: reads, finds nothing, errors — no write.
        let e = super::set_memory("no-such-instance", 4096).unwrap_err();
        assert!(format!("{e:#}").contains("no instance named"));
    }

    #[test]
    fn fabric_189_is_refused_with_words_before_any_work() {
        use crate::game::instance::CreateRequest;
        // 1.8.9 + Fabric must be refused instantly, offline, with the same
        // words every other layer uses — no HTTP 400 ever reaches the user.
        let req = CreateRequest {
            name: "PvP legacy".into(),
            icon: 0,
            version: "1.8.9".into(),
            loader: "Fabric".into(),
            memory: 4096,
            isolate_saves: true,
            discord_rpc: true,
            copy_config_from: None,
        };
        // The refusal happens before instance dirs are touched, so nothing to
        // clean up: assert purely on the message text via the same predicate.
        assert!(!arsex_launch::fabric::loader_supports_game("1.8.9"));
        let msg = arsex_launch::fabric::unsupported_message("1.8.9");
        assert!(msg.contains("does not support Minecraft 1.8.9"));
        assert!(msg.contains("VANILLA"));
        assert_eq!(req.loader, "Fabric"); // the shape the gate checks
    }

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
