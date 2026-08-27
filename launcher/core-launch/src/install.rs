//! Download, verify and lay out everything a launch needs.
//!
//! Every downloaded file is SHA-1 verified against the manifest. This is not
//! optional politeness: a truncated library jar produces a `NoClassDefFoundError`
//! deep in startup that looks like a mod bug, and users will spend hours on it.

use crate::manifest::{Library, Os, VersionJson};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const VERSION_MANIFEST: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
pub const RESOURCES: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LatestRef {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestRef,
    pub versions: Vec<ManifestEntry>,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&ManifestEntry> {
        self.versions.iter().find(|v| v.id == id)
    }
    /// Only stable releases, newest first — what the version picker shows.
    pub fn releases(&self) -> Vec<&ManifestEntry> {
        self.versions.iter().filter(|v| v.kind == "release").collect()
    }
}

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn file_sha1(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(sha1_hex(&bytes))
}

/// True when the file exists and its hash matches — lets us skip re-downloading.
pub fn verified(path: &Path, expect_sha1: &str) -> bool {
    if expect_sha1.is_empty() {
        return path.exists();
    }
    match file_sha1(path) {
        Ok(h) => h.eq_ignore_ascii_case(expect_sha1),
        Err(_) => false,
    }
}

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: String,
    pub size: u64,
}

/// Where a library's jar belongs under `libraries/`.
pub fn library_dest(lib: &Library, libraries_dir: &Path) -> Option<PathBuf> {
    if let Some(a) = &lib.downloads.artifact {
        if let Some(p) = &a.path {
            return Some(libraries_dir.join(p));
        }
    }
    lib.maven_path().map(|p| libraries_dir.join(p))
}

/// Build the download list for a resolved version: client jar, libraries and
/// natives. Anything already present and hash-verified is skipped.
pub fn plan_downloads(
    version: &VersionJson,
    libraries_dir: &Path,
    versions_dir: &Path,
    os: Os,
    features: &HashMap<String, bool>,
) -> Vec<DownloadTask> {
    let mut tasks = Vec::new();

    if let Some(client) = version.downloads.get("client") {
        let dest = versions_dir.join(&version.id).join(format!("{}.jar", version.id));
        if !verified(&dest, &client.sha1) {
            tasks.push(DownloadTask {
                url: client.url.clone(),
                dest,
                sha1: client.sha1.clone(),
                size: client.size,
            });
        }
    }

    for lib in &version.libraries {
        if !lib.applies(os, features) {
            continue;
        }
        if let (Some(a), Some(dest)) = (&lib.downloads.artifact, library_dest(lib, libraries_dir)) {
            if !verified(&dest, &a.sha1) {
                tasks.push(DownloadTask {
                    url: a.url.clone(),
                    dest,
                    sha1: a.sha1.clone(),
                    size: a.size,
                });
            }
        }
        if let Some(key) = lib.natives_key(os) {
            if let Some(a) = lib.downloads.classifiers.get(&key) {
                let rel = a.path.clone().unwrap_or_else(|| format!("{}-{}.jar", lib.name, key));
                let dest = libraries_dir.join(rel);
                if !verified(&dest, &a.sha1) {
                    tasks.push(DownloadTask {
                        url: a.url.clone(),
                        dest,
                        sha1: a.sha1.clone(),
                        size: a.size,
                    });
                }
            }
        }
    }
    tasks
}

#[derive(Debug, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
    #[serde(default)]
    pub map_to_resources: bool,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
}

impl AssetIndex {
    /// Assets are content-addressed: `objects/<first two hex chars>/<full hash>`.
    pub fn object_path(assets_dir: &Path, hash: &str) -> PathBuf {
        assets_dir.join("objects").join(&hash[..2]).join(hash)
    }

    pub fn plan(&self, assets_dir: &Path) -> Vec<DownloadTask> {
        let mut tasks = Vec::new();
        for obj in self.objects.values() {
            let dest = Self::object_path(assets_dir, &obj.hash);
            if !verified(&dest, &obj.hash) {
                tasks.push(DownloadTask {
                    url: format!("{RESOURCES}/{}/{}", &obj.hash[..2], obj.hash),
                    dest,
                    sha1: obj.hash.clone(),
                    size: obj.size,
                });
            }
        }
        tasks
    }
}

/// Extract native libraries next to the instance.
///
/// `extract.exclude` must be honoured — the entries are META-INF signatures,
/// and leaving them in place makes the JVM reject the extracted natives.
pub fn extract_natives(jar: &Path, dest: &Path, exclude: &[String]) -> Result<usize> {
    std::fs::create_dir_all(dest)?;
    let f = std::fs::File::open(jar).with_context(|| format!("opening {}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(f)?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i)?;
        let name = e.name().to_string();
        if e.is_dir() || exclude.iter().any(|x| name.starts_with(x.as_str())) {
            continue;
        }
        // Zip-slip guard: a crafted entry name must not escape `dest`.
        if name.contains("..") {
            continue;
        }
        let Some(base) = Path::new(&name).file_name() else { continue };
        let out = dest.join(base);
        let mut buf = Vec::new();
        std::io::copy(&mut e, &mut buf)?;
        std::fs::write(&out, buf)?;
        n += 1;
    }
    Ok(n)
}

/// Assemble the classpath: every applicable library, then the client jar last.
///
/// Order is load-bearing. Modloaders rely on their own classes appearing before
/// vanilla, and the client jar must come last or a patched class gets shadowed
/// by the original.
pub fn build_classpath(
    version: &VersionJson,
    libraries_dir: &Path,
    versions_dir: &Path,
    os: Os,
    features: &HashMap<String, bool>,
) -> Vec<PathBuf> {
    let mut cp = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for lib in &version.libraries {
        if !lib.applies(os, features) {
            continue;
        }
        // A library that only carries natives contributes nothing to the classpath.
        if lib.downloads.artifact.is_none() && !lib.natives.is_empty() {
            continue;
        }
        if let Some(p) = library_dest(lib, libraries_dir) {
            // Dedupe by maven group:artifact, keeping the FIRST occurrence, so
            // the modloader's pinned version wins over vanilla's.
            let key = lib.name.rsplitn(2, ':').nth(1).unwrap_or(&lib.name).to_string();
            if seen.insert(key) {
                cp.push(p);
            }
        }
    }
    cp.push(versions_dir.join(&version.id).join(format!("{}.jar", version.id)));
    cp
}

pub fn total_bytes(tasks: &[DownloadTask]) -> u64 {
    tasks.iter().map(|t| t.size).sum()
}

/// Blocking download with hash verification and atomic replace.
pub fn fetch(task: &DownloadTask, client: &reqwest::blocking::Client) -> Result<()> {
    if let Some(parent) = task.dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = client.get(&task.url).send()?.error_for_status()?.bytes()?;
    if !task.sha1.is_empty() {
        let got = sha1_hex(&bytes);
        if !got.eq_ignore_ascii_case(&task.sha1) {
            return Err(anyhow!(
                "hash mismatch for {}: expected {}, got {got}",
                task.url,
                task.sha1
            ));
        }
    }
    // Write to a temp file then rename, so an interrupted download can never
    // leave a corrupt file that passes an existence check next run.
    let tmp = task.dest.with_extension("part");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &task.dest)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Artifact, Downloads, Extract};
    use std::io::Write;

    #[test]
    fn sha1_matches_known_value() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn asset_paths_are_content_addressed() {
        let p = AssetIndex::object_path(Path::new("/a"), "deadbeefcafe");
        assert!(p.ends_with("objects/de/deadbeefcafe"));
    }

    #[test]
    fn verified_rejects_wrong_hash() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert!(verified(&p, "a9993e364706816aba3e25717850c26c9cd0d89d"));
        assert!(!verified(&p, "0000000000000000000000000000000000000000"));
        assert!(!verified(&d.path().join("nope"), "a9993e36"));
    }

    #[test]
    fn client_jar_is_last_on_classpath() {
        let v = VersionJson {
            id: "1.20.4".into(),
            main_class: "M".into(),
            libraries: vec![Library {
                name: "a.b:c:1".into(),
                downloads: Downloads {
                    artifact: Some(Artifact {
                        path: Some("a/b/c/1/c-1.jar".into()),
                        sha1: "x".into(),
                        size: 1,
                        url: "u".into(),
                    }),
                    classifiers: HashMap::new(),
                },
                rules: vec![],
                natives: HashMap::new(),
                extract: None,
                url: None,
            }],
            arguments: Default::default(),
            minecraft_arguments: None,
            asset_index: None,
            assets: None,
            downloads: HashMap::new(),
            java_version: None,
            inherits_from: None,
        };
        let cp = build_classpath(&v, Path::new("/lib"), Path::new("/ver"), Os::Windows, &HashMap::new());
        assert_eq!(cp.len(), 2);
        assert!(cp.last().unwrap().to_string_lossy().ends_with("1.20.4.jar"));
    }

    #[test]
    fn duplicate_libraries_keep_first() {
        let mk = |ver: &str| Library {
            name: format!("g:a:{ver}"),
            downloads: Downloads {
                artifact: Some(Artifact {
                    path: Some(format!("g/a/{ver}/a-{ver}.jar")),
                    sha1: "x".into(), size: 1, url: "u".into(),
                }),
                classifiers: HashMap::new(),
            },
            rules: vec![], natives: HashMap::new(), extract: None, url: None,
        };
        let v = VersionJson {
            id: "v".into(), main_class: "M".into(),
            libraries: vec![mk("2.0"), mk("1.0")],  // loader pin first
            arguments: Default::default(), minecraft_arguments: None,
            asset_index: None, assets: None, downloads: HashMap::new(),
            java_version: None, inherits_from: None,
        };
        let cp = build_classpath(&v, Path::new("/lib"), Path::new("/ver"), Os::Windows, &HashMap::new());
        // one library + client jar
        assert_eq!(cp.len(), 2);
        assert!(cp[0].to_string_lossy().contains("2.0"), "first (pinned) version must win");
    }

    #[test]
    fn natives_only_library_excluded_from_classpath() {
        let v = VersionJson {
            id: "v".into(), main_class: "M".into(),
            libraries: vec![Library {
                name: "org.lwjgl:natives:1".into(),
                downloads: Downloads { artifact: None, classifiers: HashMap::new() },
                rules: vec![],
                natives: HashMap::from([("windows".to_string(), "natives-windows".to_string())]),
                extract: None, url: None,
            }],
            arguments: Default::default(), minecraft_arguments: None,
            asset_index: None, assets: None, downloads: HashMap::new(),
            java_version: None, inherits_from: None,
        };
        let cp = build_classpath(&v, Path::new("/lib"), Path::new("/ver"), Os::Windows, &HashMap::new());
        assert_eq!(cp.len(), 1, "natives-only lib must not be on the classpath");
    }

    #[test]
    fn skips_already_verified_downloads() {
        let d = tempfile::tempdir().unwrap();
        let versions = d.path().join("versions");
        std::fs::create_dir_all(versions.join("v")).unwrap();
        let jar = versions.join("v").join("v.jar");
        std::fs::write(&jar, b"abc").unwrap();

        let mut downloads = HashMap::new();
        downloads.insert("client".to_string(), Artifact {
            path: None,
            sha1: "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
            size: 3,
            url: "u".into(),
        });
        let v = VersionJson {
            id: "v".into(), main_class: "M".into(), libraries: vec![],
            arguments: Default::default(), minecraft_arguments: None,
            asset_index: None, assets: None, downloads,
            java_version: None, inherits_from: None,
        };
        let tasks = plan_downloads(&v, &d.path().join("lib"), &versions, Os::Windows, &HashMap::new());
        assert!(tasks.is_empty(), "verified client jar should not be re-downloaded");
    }

    #[test]
    fn natives_extraction_honours_exclude() {
        let d = tempfile::tempdir().unwrap();
        let jarp = d.path().join("n.jar");
        {
            let f = std::fs::File::create(&jarp).unwrap();
            let mut z = zip::ZipWriter::new(f);
            for name in ["lwjgl.dll", "META-INF/MANIFEST.MF", "META-INF/SIG.RSA"] {
                z.start_file(name, zip::write::FileOptions::default()).unwrap();
                z.write_all(b"x").unwrap();
            }
            z.finish().unwrap();
        }
        let out = d.path().join("natives");
        let n = extract_natives(&jarp, &out, &["META-INF/".to_string()]).unwrap();
        assert_eq!(n, 1, "only the dll should be extracted");
        assert!(out.join("lwjgl.dll").exists());
        assert!(!out.join("MANIFEST.MF").exists());
    }

    #[test]
    fn zip_slip_is_blocked() {
        let d = tempfile::tempdir().unwrap();
        let jarp = d.path().join("evil.jar");
        {
            let f = std::fs::File::create(&jarp).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file("../../escaped.dll", zip::write::FileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let out = d.path().join("natives");
        let n = extract_natives(&jarp, &out, &[]).unwrap();
        assert_eq!(n, 0, "path traversal entry must be refused");
        assert!(!d.path().join("escaped.dll").exists());
    }
}
