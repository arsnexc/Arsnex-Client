//! Integration tests against the REAL Mojang API.
//!
//! These prove the parser survives production data rather than my idea of it.
//! Run with: cargo test --test live_mojang -- --ignored --nocapture

use arsex_launch::install::*;
use arsex_launch::manifest::*;
use std::collections::HashMap;

fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .user_agent("ArsexClient/2.4.1")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap()
}

#[test]
#[ignore]
fn fetches_and_parses_real_manifest() {
    let c = client();
    let m: VersionManifest = c.get(VERSION_MANIFEST).send().unwrap().json().unwrap();
    assert!(m.versions.len() > 500, "expected a large version list");
    assert!(m.find(&m.latest.release).is_some());
    assert!(m.find("1.8.9").is_some(), "1.8.9 must be present");
    println!("  manifest: {} versions, latest release {}", m.versions.len(), m.latest.release);
}

/// The important one: parse several real version JSONs spanning every format
/// era Mojang has shipped, and confirm each yields a usable launch plan.
#[test]
#[ignore]
fn parses_real_versions_across_eras() {
    let c = client();
    let m: VersionManifest = c.get(VERSION_MANIFEST).send().unwrap().json().unwrap();
    let os = Os::Windows;
    let feats: HashMap<String, bool> = HashMap::new();

    for id in ["1.8.9", "1.12.2", "1.16.5", "1.20.4", &m.latest.release.clone()] {
        let e = m.find(id).unwrap_or_else(|| panic!("version {id} missing"));
        let v: VersionJson = c.get(&e.url).send().unwrap().json().unwrap();

        assert_eq!(v.id, id);
        assert!(!v.main_class.is_empty(), "{id}: no main class");
        assert!(!v.libraries.is_empty(), "{id}: no libraries");
        assert!(v.downloads.contains_key("client"), "{id}: no client jar");
        assert!(v.asset_index.is_some(), "{id}: no asset index");

        let applicable = v.libraries.iter().filter(|l| l.applies(os, &feats)).count();
        assert!(applicable > 0, "{id}: rule engine excluded every library");
        assert!(applicable <= v.libraries.len());

        let cp = build_classpath(
            &v,
            std::path::Path::new("/lib"),
            std::path::Path::new("/ver"),
            os,
            &feats,
        );
        assert!(cp.len() > 1, "{id}: classpath too short");
        assert!(
            cp.last().unwrap().to_string_lossy().ends_with(&format!("{id}.jar")),
            "{id}: client jar must be last"
        );

        // Pre-1.13 uses minecraftArguments; 1.13+ uses the arguments block.
        let has_args = v.minecraft_arguments.is_some() || !v.arguments.game.is_empty();
        assert!(has_args, "{id}: no game arguments in either format");

        println!(
            "  {id:<8} java {:<2} · {} libs ({} apply) · {} cp · main {}",
            v.required_java(),
            v.libraries.len(),
            applicable,
            cp.len(),
            v.main_class
        );
    }
}

/// Rules must actually discriminate: a real version JSON contains
/// OS-specific natives, so Windows and Linux must resolve different sets.
#[test]
#[ignore]
fn os_rules_discriminate_on_real_data() {
    let c = client();
    let m: VersionManifest = c.get(VERSION_MANIFEST).send().unwrap().json().unwrap();
    let e = m.find("1.20.4").unwrap();
    let v: VersionJson = c.get(&e.url).send().unwrap().json().unwrap();
    let f = HashMap::new();

    let win: Vec<&str> = v.libraries.iter().filter(|l| l.applies(Os::Windows, &f))
        .map(|l| l.name.as_str()).collect();
    let lin: Vec<&str> = v.libraries.iter().filter(|l| l.applies(Os::Linux, &f))
        .map(|l| l.name.as_str()).collect();
    let osx: Vec<&str> = v.libraries.iter().filter(|l| l.applies(Os::Osx, &f))
        .map(|l| l.name.as_str()).collect();

    assert_ne!(win, lin, "windows and linux resolved identical library sets");
    println!("  1.20.4 libs — win {} · linux {} · osx {}", win.len(), lin.len(), osx.len());

    let win_only: Vec<_> = win.iter().filter(|x| !lin.contains(x)).collect();
    assert!(!win_only.is_empty(), "no windows-specific libraries found");
    println!("  windows-only examples: {:?}", &win_only[..win_only.len().min(3)]);
}

/// Build a full argv from real 1.20.4 data and verify the token never leaks
/// into anything we would log.
#[test]
#[ignore]
fn builds_real_argv_and_redacts() {
    use arsex_launch::args::{build, redact, LaunchContext};
    let c = client();
    let m: VersionManifest = c.get(VERSION_MANIFEST).send().unwrap().json().unwrap();
    let e = m.find("1.20.4").unwrap();
    let v: VersionJson = c.get(&e.url).send().unwrap().json().unwrap();

    let ctx = LaunchContext {
        player_name: "Kagemitsu".into(),
        uuid: "0123456789abcdef0123456789abcdef".into(),
        access_token: "LIVE_TOKEN_XYZ".into(),
        user_type: "msa".into(),
        version_id: "1.20.4".into(),
        version_type: "release".into(),
        game_dir: "C:/arsex/instances/main".into(),
        assets_dir: "C:/arsex/assets".into(),
        assets_index: v.asset_index.as_ref().unwrap().id.clone(),
        natives_dir: "C:/arsex/natives".into(),
        classpath: "a.jar;b.jar".into(),
        launcher_name: "arsex".into(),
        launcher_version: "2.4.1".into(),
        width: None, height: None,
        max_memory: 4096, min_memory: 512,
    };
    let argv = build(&v, &ctx, Os::Windows);
    assert!(argv.iter().any(|a| a == &v.main_class), "main class missing");
    assert!(argv.iter().any(|a| a == "--username"));
    assert!(argv.iter().any(|a| a == "Kagemitsu"));
    assert!(argv.contains(&"-Xmx4096M".to_string()));

    let joined = argv.join(" ");
    assert!(joined.contains("LIVE_TOKEN_XYZ"), "token must be in the real argv");
    let safe = redact(&argv, "LIVE_TOKEN_XYZ").join(" ");
    assert!(!safe.contains("LIVE_TOKEN_XYZ"), "token leaked into loggable output");
    assert!(!safe.contains("${"), "unsubstituted placeholder left in argv");
    println!("  argv: {} args, token redacted for logs", argv.len());
}

/// Asset index for 1.20.4 is ~4000 objects; confirm we can plan it.
#[test]
#[ignore]
fn plans_real_asset_index() {
    let c = client();
    let m: VersionManifest = c.get(VERSION_MANIFEST).send().unwrap().json().unwrap();
    let e = m.find("1.20.4").unwrap();
    let v: VersionJson = c.get(&e.url).send().unwrap().json().unwrap();
    let ai = v.asset_index.unwrap();
    let idx: AssetIndex = c.get(&ai.url).send().unwrap().json().unwrap();
    assert!(idx.objects.len() > 1000, "asset index suspiciously small");
    let tasks = idx.plan(std::path::Path::new("/tmp/arsex-assets-nonexistent"));
    assert_eq!(tasks.len(), idx.objects.len(), "nothing cached, so all must be planned");
    let gb = total_bytes(&tasks) as f64 / 1e9;
    println!("  assets: {} objects, {:.2} GB", idx.objects.len(), gb);
}

/// Actually download a small real library and verify its SHA-1.
#[test]
#[ignore]
fn downloads_and_verifies_a_real_library() {
    let c = client();
    let m: VersionManifest = c.get(VERSION_MANIFEST).send().unwrap().json().unwrap();
    let e = m.find("1.20.4").unwrap();
    let v: VersionJson = c.get(&e.url).send().unwrap().json().unwrap();

    let f = HashMap::new();
    let small = v.libraries.iter()
        .filter(|l| l.applies(Os::Windows, &f))
        .filter_map(|l| l.downloads.artifact.as_ref())
        .min_by_key(|a| a.size)
        .expect("no artifacts");

    let dir = tempfile::tempdir().unwrap();
    let task = DownloadTask {
        url: small.url.clone(),
        dest: dir.path().join("lib.jar"),
        sha1: small.sha1.clone(),
        size: small.size,
    };
    fetch(&task, &c).expect("download failed");
    assert!(verified(&task.dest, &small.sha1), "downloaded file failed verification");
    println!("  downloaded {} bytes, sha1 verified", small.size);

    // Corrupting it must be detected.
    std::fs::write(&task.dest, b"corrupted").unwrap();
    assert!(!verified(&task.dest, &small.sha1), "corruption not detected");
}
