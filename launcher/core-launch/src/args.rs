//! Launch argument construction.
//!
//! Turns a resolved `VersionJson` plus a session into the exact argv the JVM
//! receives. Two things here are security-relevant rather than cosmetic:
//!
//!   1. The access token appears in argv, which is world-readable on Windows
//!      via WMI. We cannot avoid passing it (Mojang's protocol requires it),
//!      but we CAN keep it out of logs — see `redact()`.
//!   2. Placeholder substitution must never be recursive. A player name of
//!      `${auth_access_token}` must not expand into the real token.

use crate::manifest::{Argument, Os, VersionJson};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub version_id: String,
    pub version_type: String,
    pub game_dir: String,
    pub assets_dir: String,
    pub assets_index: String,
    pub natives_dir: String,
    pub classpath: String,
    pub launcher_name: String,
    pub launcher_version: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Heap in megabytes.
    pub max_memory: u32,
    pub min_memory: u32,
    /// Official free demo: a REAL Microsoft session on an account without
    /// Java Edition entitlement, launched with `--demo`. The 1.20.4 version
    /// JSON gates `--demo` behind the `is_demo_user` feature, so setting this
    /// flag is the entire mechanism — no manual argv pushing.
    pub demo: bool,
    /// Performance mode: GC-stall guards (DisableExplicitGC), 32m G1
    /// regions, and — on heaps of 4 GB or more — AlwaysPreTouch so the heap
    /// is fully committed at start instead of soft-faulting through the
    /// first minutes of play. The pipeline pairs this with -Xms == -Xmx.
    /// These smooth frame pacing; no JVM flag multiplies raw FPS and nothing
    /// here claims to.
    pub perf: bool,
}

impl LaunchContext {
    fn placeholders(&self) -> HashMap<&'static str, String> {
        HashMap::from([
            ("auth_player_name", self.player_name.clone()),
            ("auth_uuid", self.uuid.clone()),
            ("auth_access_token", self.access_token.clone()),
            ("auth_session", format!("token:{}", self.access_token)),
            ("user_type", self.user_type.clone()),
            ("version_name", self.version_id.clone()),
            ("version_type", self.version_type.clone()),
            ("game_directory", self.game_dir.clone()),
            ("assets_root", self.assets_dir.clone()),
            ("game_assets", self.assets_dir.clone()),
            ("assets_index_name", self.assets_index.clone()),
            ("natives_directory", self.natives_dir.clone()),
            ("classpath", self.classpath.clone()),
            ("launcher_name", self.launcher_name.clone()),
            ("launcher_version", self.launcher_version.clone()),
            ("user_properties", "{}".to_string()),
            ("resolution_width", self.width.unwrap_or(854).to_string()),
            ("resolution_height", self.height.unwrap_or(480).to_string()),
            ("clientid", String::new()),
            ("auth_xuid", String::new()),
            ("library_directory", self.game_dir.clone()),
            ("classpath_separator", Os::current().cp_sep().to_string()),
        ])
    }

    pub fn features(&self) -> HashMap<String, bool> {
        HashMap::from([
            ("is_demo_user".to_string(), self.demo),
            (
                "has_custom_resolution".to_string(),
                self.width.is_some() && self.height.is_some(),
            ),
            ("has_quick_plays_support".to_string(), false),
            ("is_quick_play_singleplayer".to_string(), false),
            ("is_quick_play_multiplayer".to_string(), false),
            ("is_quick_play_realms".to_string(), false),
        ])
    }
}

/// Single-pass `${key}` substitution.
///
/// Deliberately NOT recursive: we scan the template once and copy replacement
/// text verbatim, so a value that itself contains `${...}` is never re-expanded.
/// This is what stops a hostile player name from exfiltrating the access token.
pub fn substitute(template: &str, vars: &HashMap<&'static str, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = template[i + 2..].find('}') {
                let key = &template[i + 2..i + 2 + end];
                if let Some(v) = vars.get(key) {
                    out.push_str(v);
                    i += 2 + end + 1;
                    continue;
                }
            }
        }
        // Push the raw byte as a char boundary-safe slice.
        let ch = template[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn expand(args: &[Argument], os: Os, ctx: &LaunchContext) -> Vec<String> {
    let vars = ctx.placeholders();
    let feats = ctx.features();
    let mut out = Vec::new();
    for a in args {
        match a {
            Argument::Plain(s) => out.push(substitute(s, &vars)),
            Argument::Conditional { rules, value } => {
                if crate::manifest::rules_allow(rules, os, &feats) {
                    for v in value.clone().into_vec() {
                        out.push(substitute(&v, &vars));
                    }
                }
            }
        }
    }
    out
}

/// Build the complete argv, JVM args first, then main class, then game args.
pub fn build(version: &VersionJson, ctx: &LaunchContext, os: Os) -> Vec<String> {
    let mut argv = Vec::new();

    // Heap and GC. G1 with a 50ms pause target is a materially better default
    // than the JVM's ergonomics for a game workload.
    argv.push(format!("-Xmx{}M", ctx.max_memory));
    argv.push(format!("-Xms{}M", ctx.min_memory));
    argv.push("-XX:+UseG1GC".into());
    argv.push("-XX:MaxGCPauseMillis=50".into());
    argv.push("-XX:+UnlockExperimentalVMOptions".into());
    argv.push("-XX:G1NewSizePercent=20".into());
    argv.push("-XX:G1ReservePercent=20".into());
    // Stops the JVM stalling for seconds on first launch harvesting entropy.
    argv.push("-Djava.security.egd=file:/dev/urandom".into());

    if os == Os::Osx {
        argv.push("-XstartOnFirstThread".into());
    }
    if os == Os::Windows {
        // Mojang ships this; it tunes the Windows heap allocator.
        argv.push(
            "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"
                .into(),
        );
    }

    let jvm_from_manifest = expand(&version.arguments.jvm, os, ctx);
    // Pre-1.13 versions carry no jvm argument block, so they always need the
    // essentials supplied. Critically, that stays true when a loader profile
    // is layered on top (Fabric-on-legacy): the child contributes a sparse
    // jvm block (-DFabricMcEmu=...) which must NOT displace the classpath —
    // without -cp the JVM cannot even find the main class.
    let legacy_game_args = version.minecraft_arguments.is_some();
    if legacy_game_args || jvm_from_manifest.is_empty() {
        argv.push(format!("-Djava.library.path={}", ctx.natives_dir));
        argv.push("-cp".into());
        argv.push(ctx.classpath.clone());
    }
    if !jvm_from_manifest.is_empty() {
        argv.extend(jvm_from_manifest);
    }

    // Performance mode, appended AFTER the manifest block so the dedupe sees
    // anything Mojang or a loader profile already supplied.
    if ctx.perf {
        for flag in ["-XX:G1HeapRegionSize=32m", "-XX:+DisableExplicitGC"] {
            if !argv.iter().any(|a| a == flag) {
                argv.push(flag.into());
            }
        }
        if ctx.max_memory >= 4096 && !argv.iter().any(|a| a == "-XX:+AlwaysPreTouch") {
            argv.push("-XX:+AlwaysPreTouch".into());
        }
    }

    argv.push(version.main_class.clone());

    if let Some(legacy) = &version.minecraft_arguments {
        let vars = ctx.placeholders();
        argv.extend(legacy.split_whitespace().map(|s| substitute(s, &vars)));
    } else {
        argv.extend(expand(&version.arguments.game, os, ctx));
    }

    if let (Some(w), Some(h)) = (ctx.width, ctx.height) {
        if !argv.iter().any(|a| a == "--width") {
            argv.push("--width".into());
            argv.push(w.to_string());
            argv.push("--height".into());
            argv.push(h.to_string());
        }
    }

    argv
}

/// Redact secrets for logging. The console tab and every on-disk log must run
/// through this — a support ticket containing a live session token is an
/// account takeover waiting to happen.
pub fn redact(argv: &[String], token: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    let mut next_is_token = false;
    for a in argv {
        if next_is_token {
            out.push("<redacted>".to_string());
            next_is_token = false;
            continue;
        }
        if a == "--accessToken" || a == "--session" {
            next_is_token = true;
            out.push(a.clone());
            continue;
        }
        if !token.is_empty() && a.contains(token) {
            out.push(a.replace(token, "<redacted>"));
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// Join classpath entries with the OS separator.
pub fn join_classpath(entries: &[std::path::PathBuf], os: Os) -> String {
    entries
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(os.cp_sep())
}

pub fn quote_if_needed(s: &str) -> String {
    if s.contains(' ') && !s.starts_with('"') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ArgValue, Arguments, Rule};

    fn ctx() -> LaunchContext {
        LaunchContext {
            player_name: "Kagemitsu".into(),
            uuid: "0123456789abcdef0123456789abcdef".into(),
            access_token: "SECRET_TOKEN_VALUE".into(),
            user_type: "msa".into(),
            version_id: "1.20.4".into(),
            version_type: "release".into(),
            game_dir: "C:/games/arsex".into(),
            assets_dir: "C:/games/assets".into(),
            assets_index: "12".into(),
            natives_dir: "C:/games/natives".into(),
            classpath: "a.jar;b.jar".into(),
            launcher_name: "arsex".into(),
            launcher_version: "2.5.1".into(),
            width: None,
            height: None,
            max_memory: 4096,
            min_memory: 512,
            demo: false,
            perf: false,
        }
    }

    #[test]
    fn substitutes_placeholders() {
        let c = ctx();
        let v = c.placeholders();
        assert_eq!(substitute("--username ${auth_player_name}", &v), "--username Kagemitsu");
    }

    // ---------------------------------------------------------------- demo

    /// The exact conditional argument block from the real 1.20.4 version JSON
    /// (verified against piston-meta 2026-08-30): --demo is gated behind the
    /// is_demo_user feature. The rule engine must pick it up from ctx.demo.
    fn demo_capable_version() -> VersionJson {
        let json = r#"{
            "id": "1.20.4",
            "mainClass": "net.minecraft.client.main.Main",
            "arguments": {
                "game": [
                    "${auth_player_name}",
                    {"rules": [{"action": "allow", "features": {"is_demo_user": true}}],
                     "value": "--demo"},
                    {"rules": [{"action": "allow", "features": {"has_custom_resolution": true}}],
                     "value": ["--width", "${resolution_width}", "--height", "${resolution_height}"]}
                ],
                "jvm": []
            }
        }"#;
        serde_json::from_str(json).unwrap()
    }

    fn count(argv: &[String], needle: &str) -> usize {
        argv.iter().filter(|a| a.as_str() == needle).count()
    }

    #[test]
    fn demo_flag_emits_the_real_demo_argument() {
        let v = demo_capable_version();
        let mut c = ctx();
        c.demo = true;
        let argv = build(&v, &c, Os::Windows);
        assert_eq!(count(&argv, "--demo"), 1, "demo ctx must yield --demo exactly once");
    }

    #[test]
    fn non_demo_launch_never_gets_demo_args() {
        let v = demo_capable_version();
        let argv = build(&v, &ctx(), Os::Windows);
        assert_eq!(count(&argv, "--demo"), 0, "--demo leaked into a normal launch");
    }

    #[test]
    fn demo_features_flag_is_reflected() {
        assert!(!ctx().features()["is_demo_user"]);
        let mut c = ctx();
        c.demo = true;
        assert!(c.features()["is_demo_user"]);
        // And it must not disturb neighbouring feature gates.
        assert!(!c.features()["has_custom_resolution"]);
    }

    /// A loader profile layered on a legacy version must not lose -cp: the
    /// child's sparse jvm block used to displace the essentials entirely.
    #[test]
    fn perf_mode_adds_flags_without_duplicates() {
        let mut c = ctx();
        c.perf = true;
        // 4096 MB: pre-touch included, region size + explicit-GC guard added.
        let argv = build(&demo_capable_version(), &c, Os::Windows);
        assert_eq!(argv.iter().filter(|a| *a == "-XX:+DisableExplicitGC").count(), 1);
        assert_eq!(argv.iter().filter(|a| *a == "-XX:G1HeapRegionSize=32m").count(), 1);
        assert_eq!(argv.iter().filter(|a| *a == "-XX:+AlwaysPreTouch").count(), 1);
        // The baseline G1 set is already there exactly once — perf mode must
        // not duplicate a single flag.
        for flag in ["-XX:+UseG1GC", "-XX:MaxGCPauseMillis=50"] {
            assert_eq!(argv.iter().filter(|a| *a == flag).count(), 1, "{flag} duplicated");
        }
        // Small heaps skip pre-touch: committing 2 GB up front is a bad trade.
        c.max_memory = 2048;
        let argv = build(&demo_capable_version(), &c, Os::Windows);
        assert!(!argv.contains(&"-XX:+AlwaysPreTouch".to_string()));
        // Off: none of the perf extras appear.
        let argv = build(&demo_capable_version(), &ctx(), Os::Windows);
        assert!(!argv.contains(&"-XX:+DisableExplicitGC".to_string()));
        assert!(!argv.contains(&"-XX:+AlwaysPreTouch".to_string()));
    }

    #[test]
    fn loader_on_legacy_keeps_classpath() {
        let parent: VersionJson = serde_json::from_str(
            r#"{ "id": "1.12.2", "mainClass": "net.minecraft.client.main.Main",
                "minecraftArguments": "--username ${auth_player_name} --gameDir ${game_directory}" }"#,
        ).unwrap();
        let child: VersionJson = serde_json::from_str(
            r#"{ "id": "fabric-loader-0.15.11-1.12.2",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "inheritsFrom": "1.12.2",
                "arguments": { "game": [],
                                "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "] } }"#,
        ).unwrap();
        let merged = VersionJson::inherit(child, parent);
        let argv = build(&merged, &ctx(), Os::Windows);
        assert!(argv.contains(&"-cp".to_string()), "classpath dropped: {argv:?}");
        assert!(argv.iter().any(|a| a.starts_with("-Djava.library.path=")));
        assert!(argv.iter().any(|a| a.contains("FabricMcEmu")), "loader arg lost");
        assert_eq!(argv.iter().filter(|a| a.as_str() == "--username").count(), 1);
    }

    #[test]
    fn demo_still_leaves_substitution_non_recursive() {
        let mut c = ctx();
        c.demo = true;
        c.player_name = "${auth_access_token}".into();
        let v = demo_capable_version();
        let argv = build(&v, &c, Os::Windows);
        assert!(!argv.join(" ").contains("SECRET_TOKEN_VALUE"), "token leaked");
    }

    #[test]
    fn substitution_is_not_recursive() {
        // A hostile player name must never expand into the real token.
        let mut c = ctx();
        c.player_name = "${auth_access_token}".into();
        let v = c.placeholders();
        let out = substitute("--username ${auth_player_name}", &v);
        assert_eq!(out, "--username ${auth_access_token}");
        assert!(!out.contains("SECRET_TOKEN_VALUE"), "token leaked through substitution");
    }

    #[test]
    fn unknown_placeholders_survive_verbatim() {
        let v = ctx().placeholders();
        assert_eq!(substitute("${not_a_real_key}", &v), "${not_a_real_key}");
    }

    #[test]
    fn redacts_access_token() {
        let argv = vec![
            "--username".to_string(),
            "Kagemitsu".to_string(),
            "--accessToken".to_string(),
            "SECRET_TOKEN_VALUE".to_string(),
        ];
        let r = redact(&argv, "SECRET_TOKEN_VALUE");
        assert_eq!(r[3], "<redacted>");
        assert!(!r.join(" ").contains("SECRET_TOKEN_VALUE"));
    }

    #[test]
    fn redacts_token_embedded_in_other_args() {
        let argv = vec!["--session".to_string(), "token:SECRET_TOKEN_VALUE".to_string()];
        let r = redact(&argv, "SECRET_TOKEN_VALUE");
        assert!(!r.join(" ").contains("SECRET_TOKEN_VALUE"));
    }

    #[test]
    fn legacy_argument_string_is_expanded() {
        let v = crate::manifest::VersionJson {
            id: "1.8.9".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: vec![],
            arguments: Arguments::default(),
            minecraft_arguments: Some(
                "--username ${auth_player_name} --accessToken ${auth_access_token}".into(),
            ),
            asset_index: None,
            assets: None,
            downloads: Default::default(),
            java_version: None,
            inherits_from: None,
        };
        let argv = build(&v, &ctx(), Os::Windows);
        assert!(argv.contains(&"--username".to_string()));
        assert!(argv.contains(&"Kagemitsu".to_string()));
        // main class must precede game args
        let mc = argv.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        let un = argv.iter().position(|a| a == "--username").unwrap();
        assert!(mc < un, "main class must come before game arguments");
    }

    #[test]
    fn legacy_versions_get_classpath_and_library_path() {
        let v = crate::manifest::VersionJson {
            id: "1.8.9".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: vec![],
            arguments: Arguments::default(),
            minecraft_arguments: Some("--username ${auth_player_name}".into()),
            asset_index: None,
            assets: None,
            downloads: Default::default(),
            java_version: None,
            inherits_from: None,
        };
        let argv = build(&v, &ctx(), Os::Windows);
        assert!(argv.iter().any(|a| a == "-cp"));
        assert!(argv.iter().any(|a| a.starts_with("-Djava.library.path=")));
    }

    #[test]
    fn conditional_args_respect_features() {
        let mut c = ctx();
        c.width = Some(1920);
        c.height = Some(1080);
        let v = crate::manifest::VersionJson {
            id: "1.20.4".into(),
            main_class: "M".into(),
            libraries: vec![],
            arguments: Arguments {
                game: vec![Argument::Conditional {
                    rules: vec![Rule {
                        action: "allow".into(),
                        os: None,
                        features: Some(HashMap::from([(
                            "has_custom_resolution".to_string(),
                            true,
                        )])),
                    }],
                    value: ArgValue::Many(vec![
                        "--width".into(),
                        "${resolution_width}".into(),
                    ]),
                }],
                jvm: vec![],
            },
            minecraft_arguments: None,
            asset_index: None,
            assets: None,
            downloads: Default::default(),
            java_version: None,
            inherits_from: None,
        };
        let argv = build(&v, &c, Os::Windows);
        assert!(argv.contains(&"1920".to_string()), "custom resolution arg missing");
    }

    #[test]
    fn heap_flags_present() {
        let v = crate::manifest::VersionJson {
            id: "x".into(), main_class: "M".into(), libraries: vec![],
            arguments: Arguments::default(), minecraft_arguments: Some("".into()),
            asset_index: None, assets: None, downloads: Default::default(),
            java_version: None, inherits_from: None,
        };
        let argv = build(&v, &ctx(), Os::Windows);
        assert!(argv.contains(&"-Xmx4096M".to_string()));
        assert!(argv.contains(&"-Xms512M".to_string()));
    }

    #[test]
    fn macos_gets_start_on_first_thread() {
        let v = crate::manifest::VersionJson {
            id: "x".into(), main_class: "M".into(), libraries: vec![],
            arguments: Arguments::default(), minecraft_arguments: Some("".into()),
            asset_index: None, assets: None, downloads: Default::default(),
            java_version: None, inherits_from: None,
        };
        let argv = build(&v, &ctx(), Os::Osx);
        assert!(argv.contains(&"-XstartOnFirstThread".to_string()));
    }
}
