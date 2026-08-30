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

/// The Minecraft version this build's fabric-api + Arsex mod target. The mod
/// declares `minecraft: ~1.20.4` and fabric-api is pinned to 1.20.4, so on
/// any other version both are SKIPPED (loader-only launch), never installed
/// into a game that would then crash on them.
pub const MOD_TARGET_MC: &str = "1.20.4";

/// True when the pinned fabric-api + Arsex mod can be installed.
pub fn mod_stack_supported(mc_version: &str) -> bool {
    mc_version == MOD_TARGET_MC
}

/// Does mainstream Fabric Loader support this Minecraft version at all?
///
/// Verified against meta.fabricmc.net/v2/versions/game (2026-08-30): the 520
/// supported versions start at the 1.14 era — 1.8.9 and 1.12.2 are NOT among
/// them, and the profile endpoint answers 400 for them. Mainstream Fabric
/// simply does not run on them (that is Legacy Fabric, a separate fork).
/// Saying so beats letting a bare HTTP 400 kill the launch, which is exactly
/// how "fabric 1.8.9 never launches" reports happen.
pub fn loader_supports_game(mc_version: &str) -> bool {
    let v = mc_version.trim();
    // Release ids: 1.<minor>... -> supported from 1.14. Branch suffixes
    // (1.14_combat-3) inherit their release's support.
    if let Some(rest) = v.strip_prefix("1.") {
        let minor: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(m) = minor.parse::<u32>() {
            return m >= 14;
        }
    }
    // Weekly snapshots (19w06a, 24w14a, ...): 19w06a onward is 1.14+.
    if v.len() >= 4 && v[..2].chars().all(|c| c.is_ascii_digit()) && &v[2..3] == "w" {
        let year: u32 = v[..2].parse().unwrap_or(0);
        return (19..=99).contains(&year);
    }
    false
}

/// The human explanation for an unsupported game version. Same text at every
/// layer (wizard, instance creation, launch) so the story never shifts.
pub fn unsupported_message(mc_version: &str) -> String {
    format!(
        "Fabric does not support Minecraft {mc_version} (1.14 and newer only). \
         Use the VANILLA loader for this version, or create a 1.20.4 FABRIC \
         instance for the full Arsex stack."
    )
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
    // Authoritative pre-check: fail with words, not a bare HTTP 400.
    if !loader_supports_game(mc_version) {
        anyhow::bail!("{}", unsupported_message(mc_version));
    }
    let url = format!("{FABRIC_META}/versions/loader/{mc_version}/{LOADER_VERSION}/profile/json");
    // Transient network failures must not read as "Fabric is broken": the
    // profile is a 2.8 KB fetch from a single small host, retried with
    // backoff. "Stuck at Installing Fabric loader" reports were exactly this
    // — one dropped connection, no retry, no visible error.
    let raw = fetch_profile_with_retry(client, &url, mc_version)?;
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

/// Fetch the loader profile JSON, retrying transient failures.
///
/// Retries: connection errors, send errors and 5xx/429 responses — three
/// attempts with backoff. 400/404 is authoritative ("unsupported version",
/// surfaced with words) and is NOT retried. A 4xx other than 400/404 fails
/// immediately too.
fn fetch_profile_with_retry(
    client: &reqwest::blocking::Client,
    url: &str,
    mc_version: &str,
) -> Result<Vec<u8>> {
    const ATTEMPTS: u32 = 3;
    const BACKOFF_MS: [u64; 2] = [800, 2500];
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=ATTEMPTS {
        if attempt > 1 {
            std::thread::sleep(std::time::Duration::from_millis(
                BACKOFF_MS[(attempt - 2) as usize],
            ));
        }
        let resp = match client.get(url).send() {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(
                    anyhow::Error::new(e)
                        .context("contacting fabric meta (meta.fabricmc.net)"),
                );
                continue; // connection-level failure: retry
            }
        };
        let status = resp.status().as_u16();
        if status == 400 || status == 404 {
            // The version is unsupported even though the predicate said yes —
            // surface the same human explanation either way.
            anyhow::bail!("{}", unsupported_message(mc_version));
        }
        if (500..600).contains(&status) || status == 429 {
            last_err = Some(anyhow::anyhow!("fabric meta answered HTTP {status}"));
            continue; // server-side / rate limit: retry
        }
        let raw = resp
            .error_for_status()
            .context("downloading fabric loader profile")?
            .bytes()
            .context("downloading fabric loader profile")?;
        return Ok(raw.to_vec());
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fabric meta fetch failed")))
        .with_context(|| {
            format!(
                "could not reach fabric meta after {ATTEMPTS} attempts \
                 (check your connection or a proxy blocking meta.fabricmc.net)"
            )
        })
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
    fn loader_support_table_matches_reality() {
        // Fabric's own meta (checked 2026-08-30): 1.14 era onward.
        assert!(!loader_supports_game("1.8.9"), "1.8.9 is NOT fabric-supported");
        assert!(!loader_supports_game("1.12.2"), "1.12.2 is NOT fabric-supported");
        assert!(loader_supports_game("1.14"));
        assert!(loader_supports_game("1.14_combat-3"), "branch suffix inherits support");
        assert!(loader_supports_game("1.16.5"));
        assert!(loader_supports_game("1.20.4"));
        assert!(loader_supports_game("1.21.4"));
        assert!(loader_supports_game("19w06a"), "first 1.14 snapshot");
        assert!(loader_supports_game("24w14a"));
        assert!(!loader_supports_game("18w22c"), "pre-1.14 snapshot");
        assert!(!loader_supports_game("b1.7.3"));
        assert!(!loader_supports_game(""));
    }

    #[test]
    fn mod_stack_targets_one_version_only() {
        assert!(mod_stack_supported("1.20.4"));
        assert!(!mod_stack_supported("1.20.2"), "fabric-api pin is 1.20.4-only");
        assert!(!mod_stack_supported("1.21.4"));
        assert!(!mod_stack_supported("1.8.9"));
    }

    #[test]
    fn unsupported_message_names_the_version_and_both_exits() {
        let m = unsupported_message("1.8.9");
        assert!(m.contains("1.8.9"), "{m}");
        assert!(m.contains("VANILLA"), "{m}");
        assert!(m.contains("1.20.4"), "{m}");
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
