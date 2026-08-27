//! Real mod integration.
//!
//! The prototype's "My Mods" tab derived a mod's name, loader and version from
//! its FILENAME. That is guesswork and it is wrong often enough to matter:
//! `sodium-fabric-mc1.20.1-0.5.3.jar` is not named "Sodium Fabric Mc".
//!
//! This module opens the jar and reads the metadata the mod actually declares:
//!
//!   * Fabric / Quilt -> `fabric.mod.json` / `quilt.mod.json` (JSON)
//!   * Forge 1.13+    -> `META-INF/mods.toml` (TOML)
//!   * Forge legacy   -> `mcmod.info` (JSON array)
//!
//! It also resolves dependencies and detects the two failure modes that
//! actually break a modpack: a missing hard dependency, and two mods declaring
//! the same mod id.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
    Unknown,
}

impl Loader {
    pub fn label(self) -> &'static str {
        match self {
            Loader::Fabric => "FABRIC",
            Loader::Quilt => "QUILT",
            Loader::Forge => "FORGE",
            Loader::NeoForge => "NEOFORGE",
            Loader::Unknown => "UNKNOWN",
        }
    }
    /// Fabric mods load under Quilt, but nothing else is cross-compatible.
    pub fn compatible_with(self, target: Loader) -> bool {
        self == target || (self == Loader::Fabric && target == Loader::Quilt)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub mod_id: String,
    pub required: bool,
    pub version_range: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub loader: Loader,
    /// Minecraft versions the mod declares support for, verbatim.
    pub mc_versions: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub file: PathBuf,
    pub file_size: u64,
    pub enabled: bool,
}

impl ModInfo {
    /// A disabled mod is stored as `name.jar.disabled`, matching the convention
    /// every other launcher uses, so mod folders stay portable.
    pub fn is_disabled_path(p: &Path) -> bool {
        p.extension().map(|e| e == "disabled").unwrap_or(false)
    }
}

// ---------------------------------------------------------------- fabric

#[derive(Deserialize)]
struct FabricJson {
    id: String,
    version: String,
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    authors: Vec<serde_json::Value>,
    #[serde(default)]
    depends: HashMap<String, serde_json::Value>,
    #[serde(default)]
    recommends: HashMap<String, serde_json::Value>,
}

fn parse_fabric(raw: &str, quilt: bool) -> Result<(String, String, Option<String>, Option<String>, Vec<String>, Vec<Dependency>, Vec<String>)> {
    // Quilt nests everything under quilt_loader.
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let node = if quilt {
        v.get("quilt_loader").cloned().unwrap_or(v.clone())
    } else {
        v.clone()
    };
    let f: FabricJson = serde_json::from_value(node)?;

    let authors = f
        .authors
        .iter()
        .map(|a| match a {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Object(o) => o
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            _ => "unknown".to_string(),
        })
        .collect();

    let mut deps = Vec::new();
    let mut mc = Vec::new();
    for (k, val) in f.depends.iter() {
        let range = match val {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(a) => {
                Some(a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" || "))
            }
            _ => None,
        };
        if k == "minecraft" {
            if let Some(r) = &range {
                mc.push(r.clone());
            }
            continue;
        }
        // fabricloader / java are environment constraints, not mods.
        if k == "fabricloader" || k == "java" || k == "quilt_loader" {
            continue;
        }
        deps.push(Dependency { mod_id: k.clone(), required: true, version_range: range });
    }
    for (k, val) in f.recommends.iter() {
        let range = val.as_str().map(|s| s.to_string());
        deps.push(Dependency { mod_id: k.clone(), required: false, version_range: range });
    }

    Ok((f.id, f.version, f.name, f.description, authors, deps, mc))
}

// ---------------------------------------------------------------- forge

#[derive(Deserialize)]
struct ForgeToml {
    #[serde(default)]
    mods: Vec<ForgeTomlMod>,
    #[serde(default)]
    dependencies: HashMap<String, Vec<ForgeTomlDep>>,
}

#[derive(Deserialize)]
struct ForgeTomlMod {
    #[serde(rename = "modId")]
    mod_id: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
    authors: Option<String>,
}

#[derive(Deserialize)]
struct ForgeTomlDep {
    #[serde(rename = "modId")]
    mod_id: String,
    #[serde(default)]
    mandatory: bool,
    #[serde(rename = "versionRange")]
    version_range: Option<String>,
}

fn parse_forge_toml(raw: &str) -> Result<(String, String, Option<String>, Option<String>, Vec<String>, Vec<Dependency>, Vec<String>)> {
    let t: ForgeToml = toml::from_str(raw)?;
    let m = t.mods.into_iter().next().ok_or_else(|| anyhow!("mods.toml has no [[mods]] entry"))?;

    let mut deps = Vec::new();
    let mut mc = Vec::new();
    if let Some(list) = t.dependencies.get(&m.mod_id) {
        for d in list {
            if d.mod_id == "minecraft" {
                if let Some(r) = &d.version_range {
                    mc.push(r.clone());
                }
                continue;
            }
            if d.mod_id == "forge" || d.mod_id == "neoforge" {
                continue;
            }
            deps.push(Dependency {
                mod_id: d.mod_id.clone(),
                required: d.mandatory,
                version_range: d.version_range.clone(),
            });
        }
    }

    // `${file.jarVersion}` is substituted by Forge at runtime from the jar
    // manifest; surfacing that literal to the user would be meaningless.
    let version = m
        .version
        .filter(|v| !v.contains("${"))
        .unwrap_or_else(|| "unknown".to_string());

    Ok((
        m.mod_id,
        version,
        m.display_name,
        m.description,
        m.authors.map(|a| vec![a]).unwrap_or_default(),
        deps,
        mc,
    ))
}

#[derive(Deserialize)]
struct McModInfo {
    modid: String,
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    #[serde(default, rename = "authorList")]
    author_list: Vec<String>,
    mcversion: Option<String>,
}

// ---------------------------------------------------------------- reader

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// Read a mod jar and report what it actually declares.
pub fn read_mod(path: &Path) -> Result<ModInfo> {
    let meta = std::fs::metadata(path)?;
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let (loader, raw, quilt) = if let Some(r) = read_entry(&mut zip, "quilt.mod.json") {
        (Loader::Quilt, r, true)
    } else if let Some(r) = read_entry(&mut zip, "fabric.mod.json") {
        (Loader::Fabric, r, false)
    } else if let Some(r) = read_entry(&mut zip, "META-INF/neoforge.mods.toml") {
        (Loader::NeoForge, r, false)
    } else if let Some(r) = read_entry(&mut zip, "META-INF/mods.toml") {
        (Loader::Forge, r, false)
    } else if let Some(r) = read_entry(&mut zip, "mcmod.info") {
        (Loader::Forge, r, false)
    } else {
        return Err(anyhow!(
            "no mod metadata found — not a Fabric, Quilt, Forge or NeoForge mod"
        ));
    };

    let (mod_id, version, name, description, authors, dependencies, mc_versions) = match loader {
        Loader::Fabric | Loader::Quilt => parse_fabric(&raw, quilt)?,
        Loader::Forge | Loader::NeoForge => {
            // Discriminating on `starts_with('[')` is WRONG: a mods.toml that
            // opens with `[[mods]]` also starts with '[' and would be routed
            // to the JSON parser. Try TOML first when the entry came from a
            // .toml file, and only fall back to legacy mcmod.info JSON.
            let is_legacy = raw.trim_start().starts_with('[')
                && serde_json::from_str::<Vec<McModInfo>>(&raw).is_ok();
            if is_legacy {
                let list: Vec<McModInfo> = serde_json::from_str(&raw)?;
                let m = list.into_iter().next().ok_or_else(|| anyhow!("empty mcmod.info"))?;
                (
                    m.modid,
                    m.version.unwrap_or_else(|| "unknown".into()),
                    m.name,
                    m.description,
                    m.author_list,
                    Vec::new(),
                    m.mcversion.map(|v| vec![v]).unwrap_or_default(),
                )
            } else {
                parse_forge_toml(&raw)?
            }
        }
        Loader::Unknown => unreachable!(),
    };

    Ok(ModInfo {
        name: name.unwrap_or_else(|| mod_id.clone()),
        mod_id,
        version,
        description,
        authors,
        loader,
        mc_versions,
        dependencies,
        file: path.to_path_buf(),
        file_size: meta.len(),
        enabled: !ModInfo::is_disabled_path(path),
    })
}

/// Scan a mods directory, reading every jar. Unreadable jars are reported
/// rather than silently skipped — a mod that fails to parse is exactly the one
/// the user needs to know about.
pub fn scan_dir(dir: &Path) -> (Vec<ModInfo>, Vec<(PathBuf, String)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (ok, bad);
    };
    for e in rd.flatten() {
        let p = e.path();
        let is_jar = p
            .to_string_lossy()
            .to_lowercase()
            .trim_end_matches(".disabled")
            .ends_with(".jar");
        if !is_jar {
            continue;
        }
        match read_mod(&p) {
            Ok(m) => ok.push(m),
            Err(err) => bad.push((p, err.to_string())),
        }
    }
    ok.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    (ok, bad)
}

#[derive(Debug, Clone, Serialize)]
pub struct ModProblem {
    pub kind: String,
    pub mod_id: String,
    pub detail: String,
}

/// Validate a mod set before launch. Catches the two failures that actually
/// prevent the game starting, so we can report them in the UI instead of
/// letting the user stare at a JVM stack trace.
pub fn validate(mods: &[ModInfo], target: Loader) -> Vec<ModProblem> {
    let mut problems = Vec::new();
    let enabled: Vec<&ModInfo> = mods.iter().filter(|m| m.enabled).collect();
    let present: HashSet<&str> = enabled.iter().map(|m| m.mod_id.as_str()).collect();

    let mut seen: HashMap<&str, usize> = HashMap::new();
    for m in &enabled {
        *seen.entry(m.mod_id.as_str()).or_insert(0) += 1;
    }
    for (id, n) in seen {
        if n > 1 {
            problems.push(ModProblem {
                kind: "duplicate".into(),
                mod_id: id.to_string(),
                detail: format!("{n} enabled mods declare the mod id '{id}'"),
            });
        }
    }

    for m in &enabled {
        if !m.loader.compatible_with(target) {
            problems.push(ModProblem {
                kind: "loader".into(),
                mod_id: m.mod_id.clone(),
                detail: format!(
                    "{} is a {} mod but the instance uses {}",
                    m.name,
                    m.loader.label(),
                    target.label()
                ),
            });
        }
        for d in &m.dependencies {
            if d.required && !present.contains(d.mod_id.as_str()) {
                problems.push(ModProblem {
                    kind: "missing_dependency".into(),
                    mod_id: m.mod_id.clone(),
                    detail: format!("{} requires '{}' which is not installed", m.name, d.mod_id),
                });
            }
        }
    }
    problems
}

/// Toggle a mod by renaming, the portable convention.
pub fn set_enabled(path: &Path, on: bool) -> Result<PathBuf> {
    let s = path.to_string_lossy().to_string();
    let target = if on {
        s.trim_end_matches(".disabled").to_string()
    } else if s.ends_with(".disabled") {
        s.clone()
    } else {
        format!("{s}.disabled")
    };
    if target != s {
        std::fs::rename(path, &target)?;
    }
    Ok(PathBuf::from(target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(entries: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test-mod.jar");
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (name, body) in entries {
            z.start_file(*name, zip::write::FileOptions::default()).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        (dir, p)
    }

    #[test]
    fn reads_fabric_metadata() {
        let (_d, p) = jar(&[(
            "fabric.mod.json",
            r#"{"id":"sodium","version":"0.5.3","name":"Sodium",
                "description":"Rendering engine",
                "authors":["jellysquid3"],
                "depends":{"minecraft":">=1.20","fabricloader":">=0.15"}}"#,
        )]);
        let m = read_mod(&p).unwrap();
        // The real name, not "Test Mod" derived from the filename.
        assert_eq!(m.name, "Sodium");
        assert_eq!(m.mod_id, "sodium");
        assert_eq!(m.version, "0.5.3");
        assert_eq!(m.loader, Loader::Fabric);
        assert_eq!(m.authors, vec!["jellysquid3"]);
        assert_eq!(m.mc_versions, vec![">=1.20"]);
        // fabricloader is an environment constraint, not a mod dependency
        assert!(m.dependencies.is_empty(), "loader constraint leaked into deps");
    }

    #[test]
    fn reads_fabric_object_authors() {
        let (_d, p) = jar(&[(
            "fabric.mod.json",
            r#"{"id":"x","version":"1","authors":[{"name":"Alice"}],"depends":{}}"#,
        )]);
        assert_eq!(read_mod(&p).unwrap().authors, vec!["Alice"]);
    }

    #[test]
    fn reads_forge_toml() {
        let (_d, p) = jar(&[(
            "META-INF/mods.toml",
            r#"
modLoader="javafml"
loaderVersion="[47,)"
[[mods]]
modId="jei"
version="15.2.0"
displayName="Just Enough Items"
authors="mezz"
[[dependencies.jei]]
    modId="forge"
    mandatory=true
    versionRange="[47,)"
[[dependencies.jei]]
    modId="minecraft"
    mandatory=true
    versionRange="[1.20.1,)"
"#,
        )]);
        let m = read_mod(&p).unwrap();
        assert_eq!(m.name, "Just Enough Items");
        assert_eq!(m.mod_id, "jei");
        assert_eq!(m.loader, Loader::Forge);
        assert_eq!(m.mc_versions, vec!["[1.20.1,)"]);
        assert!(m.dependencies.is_empty(), "forge/minecraft must not be listed as mod deps");
    }

    #[test]
    fn forge_jar_version_placeholder_is_not_shown() {
        let (_d, p) = jar(&[(
            "META-INF/mods.toml",
            "[[mods]]\nmodId=\"x\"\nversion=\"${file.jarVersion}\"\n",
        )]);
        assert_eq!(read_mod(&p).unwrap().version, "unknown");
    }

    #[test]
    fn reads_legacy_mcmod_info() {
        let (_d, p) = jar(&[(
            "mcmod.info",
            r#"[{"modid":"optifine","name":"OptiFine","version":"HD_U_M5","mcversion":"1.8.9"}]"#,
        )]);
        let m = read_mod(&p).unwrap();
        assert_eq!(m.name, "OptiFine");
        assert_eq!(m.mc_versions, vec!["1.8.9"]);
    }

    #[test]
    fn quilt_metadata_is_nested() {
        let (_d, p) = jar(&[(
            "quilt.mod.json",
            r#"{"quilt_loader":{"id":"qmod","version":"2.0","name":"Q Mod","depends":{}}}"#,
        )]);
        let m = read_mod(&p).unwrap();
        assert_eq!(m.mod_id, "qmod");
        assert_eq!(m.loader, Loader::Quilt);
    }

    #[test]
    fn rejects_non_mod_jar() {
        let (_d, p) = jar(&[("README.txt", "hello")]);
        assert!(read_mod(&p).is_err());
    }

    #[test]
    fn detects_missing_dependency() {
        let m = ModInfo {
            mod_id: "a".into(), name: "A".into(), version: "1".into(),
            description: None, authors: vec![], loader: Loader::Fabric,
            mc_versions: vec![],
            dependencies: vec![Dependency {
                mod_id: "missing".into(), required: true, version_range: None,
            }],
            file: PathBuf::from("a.jar"), file_size: 0, enabled: true,
        };
        let p = validate(&[m], Loader::Fabric);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].kind, "missing_dependency");
    }

    #[test]
    fn optional_dependency_is_not_a_problem() {
        let m = ModInfo {
            mod_id: "a".into(), name: "A".into(), version: "1".into(),
            description: None, authors: vec![], loader: Loader::Fabric,
            mc_versions: vec![],
            dependencies: vec![Dependency {
                mod_id: "nice_to_have".into(), required: false, version_range: None,
            }],
            file: PathBuf::from("a.jar"), file_size: 0, enabled: true,
        };
        assert!(validate(&[m], Loader::Fabric).is_empty());
    }

    #[test]
    fn detects_duplicate_mod_id() {
        let mk = |f: &str| ModInfo {
            mod_id: "dup".into(), name: "Dup".into(), version: "1".into(),
            description: None, authors: vec![], loader: Loader::Fabric,
            mc_versions: vec![], dependencies: vec![],
            file: PathBuf::from(f), file_size: 0, enabled: true,
        };
        let p = validate(&[mk("a.jar"), mk("b.jar")], Loader::Fabric);
        assert!(p.iter().any(|x| x.kind == "duplicate"));
    }

    #[test]
    fn disabled_mods_are_ignored_by_validation() {
        let mut m = ModInfo {
            mod_id: "a".into(), name: "A".into(), version: "1".into(),
            description: None, authors: vec![], loader: Loader::Forge,
            mc_versions: vec![], dependencies: vec![],
            file: PathBuf::from("a.jar"), file_size: 0, enabled: false,
        };
        m.enabled = false;
        // Forge mod in a Fabric instance, but disabled -> not a problem.
        assert!(validate(&[m], Loader::Fabric).is_empty());
    }

    #[test]
    fn loader_mismatch_detected() {
        let m = ModInfo {
            mod_id: "a".into(), name: "A".into(), version: "1".into(),
            description: None, authors: vec![], loader: Loader::Forge,
            mc_versions: vec![], dependencies: vec![],
            file: PathBuf::from("a.jar"), file_size: 0, enabled: true,
        };
        let p = validate(&[m], Loader::Fabric);
        assert!(p.iter().any(|x| x.kind == "loader"));
    }

    #[test]
    fn fabric_mods_run_on_quilt() {
        assert!(Loader::Fabric.compatible_with(Loader::Quilt));
        assert!(!Loader::Forge.compatible_with(Loader::Fabric));
    }

    #[test]
    fn toggle_renames_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("m.jar");
        std::fs::write(&p, b"x").unwrap();
        let off = set_enabled(&p, false).unwrap();
        assert!(off.to_string_lossy().ends_with(".jar.disabled"));
        assert!(off.exists() && !p.exists());
        let on = set_enabled(&off, true).unwrap();
        assert!(on.to_string_lossy().ends_with(".jar"));
        assert!(on.exists());
    }

    #[test]
    fn scan_reports_unreadable_jars() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("broken.jar"), b"not a zip").unwrap();
        let (ok, bad) = scan_dir(d.path());
        assert!(ok.is_empty());
        assert_eq!(bad.len(), 1, "a broken jar must be reported, not silently skipped");
    }
}
