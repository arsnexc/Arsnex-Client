//! Fabric loader installation: the piece that turns "a Fabric instance" from a
//! label into a game that actually loads mods.
//!
//! Before this module, choosing FABRIC in the New Instance wizard stored the
//! string and hoped. `resolve_version` only knows Mojang manifest ids, and
//! Fabric's loader profiles are not in that manifest — so a "Fabric" instance
//! silently launched vanilla and every jar in `mods/` was ignored.
//!
//! The flow here is the same one the official Fabric installer uses:
//!
//!   * the loader *profile* (a small version JSON with `inheritsFrom`) comes
//!     from Fabric's meta API and is cached under `versions/`, where
//!     `resolve_version` already knows to look;
//!   * the loader's libraries are ordinary maven coordinates that
//!     `plan_downloads` fetches and SHA-1 verify;
//!   * fabric-api and the Arsex mod are plain jars that belong in the
//!     instance's `mods/` directory.
//!
//! Everything is pinned by name in one place so a release cannot accidentally
//! ship mismatched pieces. Keep these in sync with `mod/gradle.properties`.

use crate::manifest::VersionJson;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Must match `loader_version` in `mod/gradle.properties`.
pub const LOADER_VERSION: &str = "0.15.11";

/// Must match `fabric_version` in `mod/gradle.properties`.
pub const FABRIC_API_VERSION: &str = "0.97.0+1.20.4";

/// SHA-256 of the pinned fabric-api jar (2,187,067 bytes), computed from
/// maven.fabricmc.net at pin time. The launcher must never install a
/// fabric-api it did not verify: mods execute code from these jars inside the
/// game process, so a tampered download is code execution, not a bad texture.
pub const FABRIC_API_SHA256: &str =
    "fccc011366392540b79fb26106c447caccf9b2bdb8254e823a79e07c2edda786";

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const FABRIC_MAVEN: &str = "https://maven.fabricmc.net";

/// The version id a Fabric instance is really launched with, e.g.
/// `fabric-loader-0.15.11-1.20.4`.
pub fn loader_profile_id(mc_version: &str) -> String {
    format!("fabric-loader-{LOADER_VERSION}-{mc_version}")
}

/// Fabric-api's maven download URL for the pinned version.
pub fn fabric_api_url() -> String {
    format!(
        "{FABRIC_MAVEN}/net/fabricmc/fabric-api/fabric-api/{FABRIC_API_VERSION}/\
         fabric-api-{FABRIC_API_VERSION}.jar"
    )
}

/// Fetch (or reuse) the loader profile JSON for `mc_version` under
/// `versions_dir`. Returns the profile's version id.
///
/// The file is written the way `resolve_version` expects —
/// `versions/<id>/<id>.json` — via temp file + atomic rename, and only after
/// the payload parses as a version JSON, so a truncated download or an HTML
/// error page can never wedge future launches.
pub fn ensure_loader_profile(
    client: &reqwest::blocking::Client,
    mc_version: &str,
    versions_dir: &Path,
) -> Result<String> {
    let id = loader_profile_id(mc_version);
    let dest = versions_dir.join(&id).join(format!("{id}.json"));
    if dest.exists() {
        // Present. If it is corrupt, the launch fails with a parse error that
        // names the file; silently re-fetching would hide a flaky disk.
        return Ok(id);
    }
    let url = format!("{FABRIC_META}/versions/loader/{mc_version}/{LOADER_VERSION}/profile/json");
    let raw = client
        .get(&url)
        .send()?
        .error_for_status()?
        .bytes()
        .context("downloading fabric loader profile")?;
    let parsed: VersionJson = serde_json::from_slice(&raw)
        .with_context(|| format!("fabric meta returned an unparseable profile for {mc_version}"))?;
    if parsed.main_class.is_empty() {
        anyhow::bail!("fabric loader profile for {mc_version} has no mainClass");
    }
    std::fs::create_dir_all(dest.parent().unwrap())?;
    let tmp = dest.with_extension("json.tmp");
    std::fs::write(&tmp, &raw)?;
    std::fs::rename(&tmp, &dest)?;
    Ok(id)
}

/// Fetch (or reuse, hash-verified) the pinned fabric-api jar under
/// `cache_dir/mods/`. Returns the path of the verified jar.
pub fn ensure_fabric_api(client: &reqwest::blocking::Client, cache_dir: &Path) -> Result<PathBuf> {
    let dir = cache_dir.join("mods");
    let dest = dir.join(format!("fabric-api-{FABRIC_API_VERSION}.jar"));
    if let Ok(bytes) = std::fs::read(&dest) {
        if sha256_hex(&bytes).eq_ignore_ascii_case(FABRIC_API_SHA256) {
            return Ok(dest);
        }
        // Right name, wrong bytes: it must not survive.
        let _ = std::fs::remove_file(&dest);
    }
    let bytes = client
        .get(fabric_api_url())
        .send()?
        .error_for_status()?
        .bytes()
        .context("downloading fabric-api")?;
    let got = sha256_hex(&bytes);
    if !got.eq_ignore_ascii_case(FABRIC_API_SHA256) {
        anyhow::bail!(
            "fabric-api {FABRIC_API_VERSION} hash mismatch: expected {FABRIC_API_SHA256}, got {got}"
        );
    }
    std::fs::create_dir_all(&dir)?;
    let tmp = dest.with_extension("jar.tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Install a jar into an instance's `mods/` directory, skipping the copy only
/// when the destination already has exactly these bytes.
///
/// Returns the destination path and whether a copy happened. Content compare,
/// not mtime: this is also how the embedded Arsex mod upgrades itself when the
/// launcher ships a new version next to an old jar.
pub fn install_mod_jar(bytes: &[u8], mods_dir: &Path, file_name: &str) -> Result<(PathBuf, bool)> {
    std::fs::create_dir_all(mods_dir)?;
    let dest = mods_dir.join(file_name);
    if let Ok(existing) = std::fs::read(&dest) {
        if existing == bytes {
            return Ok((dest, false));
        }
    }
    let tmp = dest.with_extension("jar.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &dest)?;
    Ok((dest, true))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the real meta.fabricmc.net response for 1.20.4 / 0.15.11,
    /// shape preserved exactly — including the fact that fabric-loader and
    /// intermediary carry NO sha1/size (only the asm/mixin libs do; the asm
    /// sha1 below is the real one). Enough to prove every property the
    /// launcher depends on.
    const PROFILE: &str = r#"{
        "id": "fabric-loader-0.15.11-1.20.4",
        "inheritsFrom": "1.20.4",
        "releaseTime": "2024-06-01T00:00:00+0000",
        "time": "2024-06-01T00:00:00+0000",
        "type": "release",
        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "arguments": { "game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "] },
        "libraries": [
            {
                "name": "org.ow2.asm:asm:9.6",
                "url": "https://maven.fabricmc.net/",
                "sha1": "aa205cf0a06dbd8e04ece91c0b37c3f5d567546a",
                "size": 123598
            },
            {
                "name": "net.fabricmc:fabric-loader:0.15.11",
                "url": "https://maven.fabricmc.net/"
            }
        ]
    }"#;

    #[test]
    fn profile_id_matches_the_pinned_loader() {
        assert_eq!(loader_profile_id("1.20.4"), "fabric-loader-0.15.11-1.20.4");
    }

    #[test]
    fn fabric_api_url_is_well_formed() {
        let url = fabric_api_url();
        assert!(url.starts_with("https://maven.fabricmc.net/"), "{url}");
        assert!(url.ends_with("/fabric-api-0.97.0+1.20.4.jar"), "{url}");
    }

    #[test]
    fn loader_profile_parses_with_flat_libraries() {
        let v: VersionJson = serde_json::from_str(PROFILE).expect("real profile shape must parse");
        assert_eq!(v.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(v.inherits_from.as_deref(), Some("1.20.4"));
        assert_eq!(v.libraries.len(), 2);
        let asm = &v.libraries[0];
        assert_eq!(asm.name, "org.ow2.asm:asm:9.6");
        assert_eq!(asm.sha1.as_deref(), Some("aa205cf0a06dbd8e04ece91c0b37c3f5d567546a"));
        let loader = &v.libraries[1];
        // Fabric's meta genuinely omits sha1/size for these two; the launcher
        // must cope rather than drop them.
        assert_eq!(loader.name, "net.fabricmc:fabric-loader:0.15.11");
        assert_eq!(loader.sha1, None);
        assert_eq!(
            loader.maven_path().as_deref(),
            Some("net/fabricmc/fabric-loader/0.15.11/fabric-loader-0.15.11.jar")
        );
    }

    #[test]
    fn profile_inherits_parent_libraries_and_main_class() {
        // The child's KnotClient must win; loader libraries must come first on
        // the classpath; vanilla libraries must survive the merge.
        let child: VersionJson = serde_json::from_str(PROFILE).unwrap();
        let parent: VersionJson = serde_json::from_str(
            r#"{
                "id": "1.20.4",
                "mainClass": "net.minecraft.client.main.Main",
                "libraries": [{
                    "name": "com.mojang:brigadier:1.0.18",
                    "downloads": { "artifact": {
                        "path": "com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar",
                        "sha1": "6aaea8c099d107e78a74d15f30c59bd5a7e17c8d",
                        "size": 1,
                        "url": "https://libraries.minecraft.net/com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar"
                    }}
                }]
            }"#,
        )
        .unwrap();
        let merged = VersionJson::inherit(child, parent);
        assert_eq!(merged.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(merged.libraries.len(), 3);
        assert_eq!(merged.libraries[0].name, "org.ow2.asm:asm:9.6");
        assert_eq!(merged.libraries[1].name, "net.fabricmc:fabric-loader:0.15.11");
        assert_eq!(merged.libraries[2].name, "com.mojang:brigadier:1.0.18");
    }

    #[test]
    fn flat_fabric_libraries_become_download_tasks() {
        use crate::install::plan_downloads;
        use std::collections::HashMap;

        let v: VersionJson = serde_json::from_str(PROFILE).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let tasks = plan_downloads(
            &v,
            dir.path(),
            dir.path(),
            crate::manifest::Os::Windows,
            &HashMap::new(),
        );
        assert_eq!(tasks.len(), 2, "both flat fabric libs must be planned: {tasks:?}");
        let asm = tasks.iter().find(|t| t.url.contains("asm-9.6")).unwrap();
        assert_eq!(
            asm.url,
            "https://maven.fabricmc.net/org/ow2/asm/asm/9.6/asm-9.6.jar"
        );
        assert_eq!(asm.sha1, "aa205cf0a06dbd8e04ece91c0b37c3f5d567546a");
        let loader = tasks
            .iter()
            .find(|t| t.url.contains("fabric-loader"))
            .unwrap();
        assert_eq!(
            loader.url,
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.11/fabric-loader-0.15.11.jar"
        );
        // No sha1 in the profile -> empty string -> download is not hash-gated
        // (same trust level as the official installer: HTTPS + meta-pinned path).
        assert_eq!(loader.sha1, "");
    }

    #[test]
    fn merged_fabric_version_classpath_includes_both_sides() {
        use crate::install::build_classpath;
        use std::collections::HashMap;

        let child: VersionJson = serde_json::from_str(PROFILE).unwrap();
        let parent: VersionJson = serde_json::from_str(
            r#"{ "id": "1.20.4", "mainClass": "net.minecraft.client.main.Main",
                 "libraries": [{
                    "name": "com.mojang:brigadier:1.0.18",
                    "downloads": { "artifact": {
                        "path": "com/mojang/brigadier/1.0.18/brigadier-1.0.18.jar",
                        "sha1": "6aaea8c099d107e78a74d15f30c59bd5a7e17c8d",
                        "size": 1,
                        "url": "https://libraries.minecraft.net/x.jar"
                    }}
                 }] }"#,
        )
        .unwrap();
        let merged = VersionJson::inherit(child, parent);
        let dir = tempfile::tempdir().unwrap();
        let cp = build_classpath(
            &merged,
            dir.path(),
            dir.path(),
            crate::manifest::Os::Windows,
            &HashMap::new(),
        );
        assert!(cp.iter().any(|p| p.ends_with("fabric-loader-0.15.11.jar")),
            "loader jar missing from classpath: {cp:?}");
        assert!(cp.iter().any(|p| p.ends_with("brigadier-1.0.18.jar")),
            "vanilla jar missing from classpath: {cp:?}");
    }

    #[test]
    fn install_mod_jar_is_idempotent_and_upgrading() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        let (p1, wrote1) = install_mod_jar(b"v1", &mods, "arsex-mod-2.5.0.jar").unwrap();
        assert!(wrote1 && p1.exists());
        let (_, wrote2) = install_mod_jar(b"v1", &mods, "arsex-mod-2.5.0.jar").unwrap();
        assert!(!wrote2, "identical bytes must not rewrite");
        let _ = install_mod_jar(b"v2-longer", &mods, "arsex-mod-2.5.0.jar").unwrap();
        assert_eq!(std::fs::read(p1).unwrap(), b"v2-longer", "new bytes must replace");
        let leftover: Vec<_> = std::fs::read_dir(&mods)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(leftover.is_empty(), "temp files must not survive: {leftover:?}");
    }

    #[test]
    fn sha256_hex_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
