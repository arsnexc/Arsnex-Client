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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

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
        // Two library dialects exist:
        //   Mojang: downloads.artifact.{path,url,sha1,size}
        //   Fabric: flat {name, url, sha1?, size?} + maven coordinates
        let flat = if lib.downloads.artifact.is_none() {
            match (&lib.url, lib.maven_path()) {
                (Some(base), Some(rel)) => Some((
                    format!("{}/{}", base.trim_end_matches('/'), rel),
                    lib.sha1.clone().unwrap_or_default(),
                    lib.size.unwrap_or(0),
                )),
                _ => None,
            }
        } else {
            None
        };
        if let (Some(a), Some(dest)) = (&lib.downloads.artifact, library_dest(lib, libraries_dir)) {
            if !verified(&dest, &a.sha1) {
                tasks.push(DownloadTask {
                    url: a.url.clone(),
                    dest,
                    sha1: a.sha1.clone(),
                    size: a.size,
                });
            }
        } else if let Some((url, sha1, size)) = flat {
            let dest = libraries_dir.join(
                lib.maven_path().expect("flat libs are only planned when a path exists"),
            );
            // An empty sha1 (fabric-loader/intermediary ship without one)
            // means the download is not hash-gated — verified() then only
            // checks existence, matching what the official installer does.
            if !verified(&dest, &sha1) {
                tasks.push(DownloadTask { url, dest, sha1, size });
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

    /// Stamp written after a fully successful asset pass for `index_id`.
    /// While it exists, later launches plan assets by existence+size instead
    /// of re-hashing ~500 MB — the warm-launch path. Safe because asset
    /// storage is CONTENT-ADDRESSed: a changed asset has a different hash,
    /// therefore a different path, therefore a miss and a real download.
    pub fn stamp_path(assets_dir: &Path, index_id: &str) -> PathBuf {
        assets_dir.join("indexes").join(format!("{index_id}.ok"))
    }

    /// `fast` uses existence+size instead of a full SHA-1 read (see
    /// [`AssetIndex::stamp_path`] for why that is sound).
    pub fn plan(&self, assets_dir: &Path, fast: bool) -> Vec<DownloadTask> {
        let mut tasks = Vec::new();
        for obj in self.objects.values() {
            let dest = Self::object_path(assets_dir, &obj.hash);
            let present = if fast {
                std::fs::metadata(&dest).map(|m| m.len() == obj.size).unwrap_or(false)
            } else {
                verified(&dest, &obj.hash)
            };
            if !present {
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
/// Transient server conditions worth another attempt.
fn retryable_status(status: u16) -> bool {
    (500..600).contains(&status) || status == 429
}

/// Download one file, retrying transient failures (connection errors, 5xx,
/// 429) up to three attempts with short backoff. Until v2.7.1 a single
/// dropped connection anywhere in a ~4000-file asset pass failed the whole
/// creation — and creation cleanup then deleted everything.
pub fn fetch(task: &DownloadTask, client: &reqwest::blocking::Client) -> Result<()> {
    const ATTEMPTS: u32 = 3;
    const BACKOFF_MS: [u64; 2] = [400, 1200];
    if let Some(parent) = task.dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=ATTEMPTS {
        if attempt > 1 {
            std::thread::sleep(std::time::Duration::from_millis(
                BACKOFF_MS[(attempt - 2) as usize],
            ));
        }
        let resp = match client.get(&task.url).send() {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(anyhow::Error::new(e).context(format!("requesting {}", task.url)));
                continue; // connection-level failure: retry
            }
        };
        if retryable_status(resp.status().as_u16()) {
            last_err = Some(anyhow!("HTTP {} for {}", resp.status().as_u16(), task.url));
            continue; // server-side / rate limit: retry
        }
        let bytes = match resp.error_for_status().and_then(|r| r.bytes().map_err(Into::into)) {
            Ok(b) => b,
            Err(e) => return Err(anyhow::Error::new(e).context(format!("downloading {}", task.url))),
        };
        if !task.sha1.is_empty() {
            let got = sha1_hex(&bytes);
            if !got.eq_ignore_ascii_case(&task.sha1) {
                // Correct-route-but-wrong-bytes is corruption, not transience.
                // Never retry it, never write it.
                return Err(anyhow!(
                    "hash mismatch for {}: expected {}, got {got}",
                    task.url,
                    task.sha1
                ));
            }
        }
        // Write to a temp file then rename, so an interrupted download can
        // never leave a corrupt file that passes an existence check next run.
        let tmp = task.dest.with_extension("part");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &task.dest)?;
        return Ok(());
    }
    Err(last_err
        .unwrap_or_else(|| anyhow!("download failed: {}", task.url))
        .context(format!("giving up on {} after {ATTEMPTS} attempts", task.url)))
}

/// Concurrency for asset/library passes. Six parallel connections is the
/// sweet spot observed by mainstream launchers: it lifts a 4000-file asset
/// pass from minutes to tens of seconds without hammering Mojang's CDN.
pub const FETCH_WORKERS: usize = 6;

/// Download every task, up to [`FETCH_WORKERS`] at a time, each with the
/// per-file retry of [`fetch`].
///
/// `progress(files_done, files_total, bytes_done, bytes_total)` is called
/// from worker threads — throttled to every 25 files plus once at the end —
/// so the UI never floods. On the first failure the workers stop STARTING
/// new files (in-flight ones finish), and that first error is returned with
/// its URL attached.
pub fn fetch_all(
    client: &reqwest::blocking::Client,
    tasks: &[DownloadTask],
    progress: &(dyn Fn(usize, usize, u64, u64) + Sync),
) -> Result<()> {
    let total_files = tasks.len();
    let total_bytes = total_bytes(tasks).max(1);
    if total_files == 0 {
        progress(0, 0, 0, 0);
        return Ok(());
    }
    let next = AtomicUsize::new(0);
    let done_files = AtomicUsize::new(0);
    let done_bytes = AtomicU64::new(0);
    let abort = AtomicBool::new(false);
    let first_err: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let workers = FETCH_WORKERS.min(total_files);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                if abort.load(Ordering::Relaxed) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total_files {
                    break;
                }
                let t = &tasks[i];
                match fetch(t, client) {
                    Ok(()) => {
                        let f = done_files.fetch_add(1, Ordering::Relaxed) + 1;
                        let b = done_bytes.fetch_add(t.size, Ordering::Relaxed) + t.size;
                        if f % 25 == 0 || f == total_files {
                            progress(f, total_files, b, total_bytes);
                        }
                    }
                    Err(e) => {
                        let mut slot = first_err.lock().unwrap();
                        if slot.is_none() {
                            *slot = Some(e);
                        }
                        abort.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
        }
    });
    if let Some(e) = first_err.into_inner().unwrap() {
        return Err(e);
    }
    progress(
        done_files.load(Ordering::Relaxed),
        total_files,
        done_bytes.load(Ordering::Relaxed),
        total_bytes,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- local HTTP server for the download engine -----------------------

    /// Spin a one-thread tiny_http server. `handler(url, hits) -> (status, body)`
    /// decides each response; `hits` counts requests per path.
    fn serve<F>(handler: F) -> (String, std::sync::Arc<Mutex<HashMap<String, usize>>>)
    where
        F: Fn(&str, usize) -> (u16, String) + Send + Sync + 'static,
    {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_string();
        let hits: std::sync::Arc<Mutex<HashMap<String, usize>>> =
            std::sync::Arc::new(Mutex::new(HashMap::new()));
        let h = std::sync::Arc::new(handler);
        let hits2 = hits.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let key = req.url().to_string();
                let n = {
                    let mut m = hits2.lock().unwrap();
                    let e = m.entry(key.clone()).or_insert(0);
                    *e += 1;
                    *e
                };
                let (code, body) = h(&key, n);
                let _ = req.respond(
                    tiny_http::Response::from_string(body).with_status_code(code),
                );
            }
        });
        (format!("http://{addr}"), hits)
    }

    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap()
    }

    #[test]
    fn fetch_retries_a_transient_5xx_and_lands_the_file() {
        let body = "asset-bytes".to_string();
        let want = sha1_hex(body.as_bytes());
        let b = body.clone();
        let (base, hits) = serve(move |url, n| {
            if url.contains("flaky") && n < 3 {
                (500, String::new()) // first two attempts fail server-side
            } else {
                (200, b.clone())
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let t = DownloadTask {
            url: format!("{base}/flaky/obj"),
            dest: dir.path().join("obj"),
            sha1: want,
            size: body.len() as u64,
        };
        fetch(&t, &client()).unwrap();
        assert_eq!(std::fs::read(t.dest).unwrap(), body.as_bytes());
        assert_eq!(
            hits.lock().unwrap().get("/flaky/obj"),
            Some(&3),
            "must have taken exactly three attempts"
        );
    }

    #[test]
    fn fetch_never_retries_a_hash_mismatch() {
        let (base, hits) = serve(|_u, _n| (200, "these-are-the-wrong-bytes".to_string()));
        let dir = tempfile::tempdir().unwrap();
        let t = DownloadTask {
            url: format!("{base}/x"),
            dest: dir.path().join("x"),
            sha1: sha1_hex(b"expected"),
            size: 4,
        };
        let e = fetch(&t, &client()).unwrap_err();
        assert!(format!("{e:#}").contains("hash mismatch"), "{e:#}");
        assert_eq!(hits.lock().unwrap().values().sum::<usize>(), 1);
        assert!(!t.dest.exists(), "corrupt bytes must never be written");
    }

    #[test]
    fn fetch_all_runs_every_task_and_reports_totals() {
        let n_files = 40usize;
        let (base, _hits) = serve(move |url, _n| {
            // Distinct, deterministic body per path: /7 -> "body-7-...".
            let name = url.trim_start_matches('/');
            let body = format!("body-{name}-padpadpadpad");
            (200, body)
        });
        let dir = tempfile::tempdir().unwrap();
        let mut tasks = Vec::new();
        for i in 0..n_files {
            let body = format!("body-{i}-padpadpadpad");
            tasks.push(DownloadTask {
                url: format!("{base}/{i}"),
                dest: dir.path().join(i.to_string()),
                sha1: sha1_hex(body.as_bytes()),
                size: body.len() as u64,
            });
        }
        let seen: std::sync::Arc<Mutex<Vec<(usize, usize, u64, u64)>>> =
            std::sync::Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        fetch_all(&client(), &tasks, &move |f, t, b, tb| {
            seen2.lock().unwrap().push((f, t, b, tb));
        })
        .unwrap();
        for i in 0..n_files {
            assert!(dir.path().join(i.to_string()).exists(), "file {i} missing");
        }
        let final_p = *seen.lock().unwrap().last().unwrap();
        assert_eq!(final_p.0, n_files);
        assert_eq!(final_p.1, n_files);
        assert_eq!(final_p.2, final_p.3, "bytes done must equal total");
    }

    #[test]
    fn fetch_all_returns_the_first_error_with_its_url() {
        let (base, _hits) = serve(|url, _n| {
            if url.contains("poison") {
                (404, String::new()) // permanent: not retried, fails the run
            } else {
                (200, "ok".to_string())
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let tasks: Vec<DownloadTask> = (0..12)
            .map(|i| DownloadTask {
                url: format!("{base}/{}", if i == 5 { "poison".to_string() } else { i.to_string() }),
                dest: dir.path().join(i.to_string()),
                sha1: String::new(),
                size: 2,
            })
            .collect();
        let e = fetch_all(&client(), &tasks, &|_, _, _, _| {}).unwrap_err();
        assert!(format!("{e:#}").contains("poison"), "{e:#}");
    }

    #[test]
    fn asset_fast_plan_skips_only_size_matched_files() {
        let dir = tempfile::tempdir().unwrap();
        let good = b"good-asset-bytes".to_vec();
        let hash = sha1_hex(&good);
        let mut objects = HashMap::new();
        objects.insert(
            "good".to_string(),
            AssetObject { hash: hash.clone(), size: good.len() as u64 },
        );
        objects.insert(
            "short".to_string(),
            AssetObject { hash: sha1_hex(b"other-bytes"), size: 99 }, // wrong size on disk
        );
        let idx = AssetIndex { objects, map_to_resources: false, is_virtual: false };
        // Warm: the good asset exists with the right size -> skipped.
        let dest = AssetIndex::object_path(dir.path(), &hash);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, &good).unwrap();
        let short_dest = AssetIndex::object_path(dir.path(), &sha1_hex(b"other-bytes"));
        std::fs::create_dir_all(short_dest.parent().unwrap()).unwrap();
        std::fs::write(&short_dest, b"tiny").unwrap(); // size mismatch
        let fast = idx.plan(dir.path(), true);
        assert_eq!(fast.len(), 1, "only the size-mismatched asset is queued");
        assert_eq!(fast[0].sha1, sha1_hex(b"other-bytes"));
        // Cold path still hash-verifies: the good asset is skipped for real.
        let cold = idx.plan(dir.path(), false);
        assert_eq!(cold.len(), 1, "hash path agrees on what is missing");
    }

    use crate::manifest::{Artifact, Downloads};
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
                sha1: None,
                size: None,
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
            sha1: None, size: None,
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
                sha1: None,
                size: None,
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
