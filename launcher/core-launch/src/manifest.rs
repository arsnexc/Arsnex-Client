//! Mojang version manifest parsing, rule evaluation and library resolution.
//!
//! This is the part that decides *what a launch actually consists of*: which
//! jars land on the classpath, which natives get extracted, which arguments
//! the JVM receives. Getting the rule engine wrong is the single most common
//! reason a third-party launcher produces a game that crashes on startup.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------- os

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Osx,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Osx
        } else {
            Os::Linux
        }
    }
    pub fn mojang_name(self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::Osx => "osx",
        }
    }
    /// Classpath separator. Windows uses `;`, everything else `:`.
    /// Getting this wrong yields "Could not find or load main class".
    pub fn cp_sep(self) -> &'static str {
        match self {
            Os::Windows => ";",
            _ => ":",
        }
    }
}

// ---------------------------------------------------------------- rules

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub version: Option<String>,
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
    pub features: Option<HashMap<String, bool>>,
}

/// Mojang's rule semantics, which are easy to get subtly wrong:
///
/// * Rules are evaluated **in order**; the last match wins.
/// * The default when a rule list is present is **disallow** — an entry with
///   rules is excluded unless something explicitly allows it.
/// * An empty rule list means allow.
/// * Feature rules (`is_demo_user`, `has_custom_resolution`) must be matched
///   against the launch context, not assumed false, or custom-resolution args
///   silently vanish.
pub fn rules_allow(rules: &[Rule], os: Os, features: &HashMap<String, bool>) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, os, features) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn rule_matches(rule: &Rule, os: Os, features: &HashMap<String, bool>) -> bool {
    if let Some(o) = &rule.os {
        if let Some(name) = &o.name {
            if name != os.mojang_name() {
                return false;
            }
        }
        if let Some(arch) = &o.arch {
            // Mojang only ever emits x86 here, meaning 32-bit.
            let current = if cfg!(target_pointer_width = "32") { "x86" } else { "x86_64" };
            if arch != current {
                return false;
            }
        }
    }
    if let Some(want) = &rule.features {
        for (k, v) in want {
            if features.get(k).copied().unwrap_or(false) != *v {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------- artifacts

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Artifact {
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Downloads {
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: HashMap<String, Artifact>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Downloads,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Legacy 1.8-era natives map: os name -> classifier key.
    #[serde(default)]
    pub natives: HashMap<String, String>,
    #[serde(default)]
    pub extract: Option<Extract>,
    /// Forge/Fabric libraries often carry only a base url.
    pub url: Option<String>,
    /// Fabric-style profiles also put sha1/size at the top level instead of
    /// inside `downloads.artifact`. Fabric's meta omits both for
    /// fabric-loader and intermediary themselves, so they stay optional.
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Library {
    /// Maven coordinate -> relative path, e.g.
    /// `com.mojang:patchy:1.1` -> `com/mojang/patchy/1.1/patchy-1.1.jar`.
    /// Coordinates may carry a classifier: `group:artifact:version:classifier`.
    pub fn maven_path(&self) -> Option<String> {
        let mut parts = self.name.split(':');
        let group = parts.next()?.replace('.', "/");
        let artifact = parts.next()?;
        let version = parts.next()?;
        let classifier = parts.next();
        let file = match classifier {
            Some(c) => format!("{artifact}-{version}-{c}.jar"),
            None => format!("{artifact}-{version}.jar"),
        };
        Some(format!("{group}/{artifact}/{version}/{file}"))
    }

    pub fn applies(&self, os: Os, features: &HashMap<String, bool>) -> bool {
        rules_allow(&self.rules, os, features)
    }

    /// The natives classifier for this OS, if this library ships natives.
    pub fn natives_key(&self, os: Os) -> Option<String> {
        let raw = self.natives.get(os.mojang_name())?;
        // Mojang templates the architecture into the key.
        let arch = if cfg!(target_pointer_width = "32") { "32" } else { "64" };
        Some(raw.replace("${arch}", arch))
    }
}

// ---------------------------------------------------------------- arguments

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgValue,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl ArgValue {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            ArgValue::One(s) => vec![s],
            ArgValue::Many(v) => v,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

// ---------------------------------------------------------------- version

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(rename = "totalSize")]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JavaVersion {
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Arguments,
    /// Pre-1.13 single-string argument form.
    #[serde(rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(rename = "assetIndex")]
    pub asset_index: Option<AssetIndexRef>,
    pub assets: Option<String>,
    #[serde(default)]
    pub downloads: HashMap<String, Artifact>,
    #[serde(rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
    /// Set by Forge/Fabric profiles that layer onto a vanilla version.
    #[serde(rename = "inheritsFrom")]
    pub inherits_from: Option<String>,
}

impl VersionJson {
    /// Merge a modloader profile onto its parent vanilla version.
    ///
    /// Order matters enormously: the loader's libraries must come FIRST on the
    /// classpath so its patched classes shadow vanilla ones. Reversing this
    /// produces a game that launches and then behaves as if no mods loaded.
    pub fn inherit(child: VersionJson, parent: VersionJson) -> VersionJson {
        let mut libs = child.libraries.clone();
        libs.extend(parent.libraries.clone());

        let mut args = Arguments::default();
        args.game = parent.arguments.game.clone();
        args.game.extend(child.arguments.game.clone());
        args.jvm = parent.arguments.jvm.clone();
        args.jvm.extend(child.arguments.jvm.clone());

        VersionJson {
            id: child.id,
            main_class: child.main_class,
            libraries: libs,
            arguments: args,
            minecraft_arguments: child
                .minecraft_arguments
                .or(parent.minecraft_arguments),
            asset_index: child.asset_index.or(parent.asset_index),
            assets: child.assets.or(parent.assets),
            downloads: if child.downloads.is_empty() {
                parent.downloads
            } else {
                child.downloads
            },
            java_version: child.java_version.or(parent.java_version),
            inherits_from: None,
        }
    }

    /// Required Java major version. Mojang omits this pre-1.17, where 8 is correct.
    pub fn required_java(&self) -> u32 {
        self.java_version.as_ref().map(|j| j.major_version).unwrap_or(8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feats() -> HashMap<String, bool> {
        HashMap::new()
    }

    #[test]
    fn empty_rules_allow() {
        assert!(rules_allow(&[], Os::Windows, &feats()));
    }

    #[test]
    fn rules_default_to_disallow() {
        // A rule that only allows osx must exclude windows.
        let r = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule { name: Some("osx".into()), version: None, arch: None }),
            features: None,
        }];
        assert!(rules_allow(&r, Os::Osx, &feats()));
        assert!(!rules_allow(&r, Os::Windows, &feats()));
    }

    #[test]
    fn last_matching_rule_wins() {
        // allow all, then disallow osx -> osx excluded, windows kept.
        let r = vec![
            Rule { action: "allow".into(), os: None, features: None },
            Rule {
                action: "disallow".into(),
                os: Some(OsRule { name: Some("osx".into()), version: None, arch: None }),
                features: None,
            },
        ];
        assert!(rules_allow(&r, Os::Windows, &feats()));
        assert!(!rules_allow(&r, Os::Osx, &feats()));
    }

    #[test]
    fn feature_rules_respect_context() {
        let r = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some(HashMap::from([("has_custom_resolution".to_string(), true)])),
        }];
        assert!(!rules_allow(&r, Os::Windows, &feats()));
        let on = HashMap::from([("has_custom_resolution".to_string(), true)]);
        assert!(rules_allow(&r, Os::Windows, &on));
    }

    #[test]
    fn maven_coordinates_resolve() {
        let l = Library {
            name: "com.mojang:patchy:1.1".into(),
            downloads: Downloads::default(),
            rules: vec![],
            natives: HashMap::new(),
            extract: None,
            url: None,
            sha1: None,
            size: None,
        };
        assert_eq!(l.maven_path().unwrap(), "com/mojang/patchy/1.1/patchy-1.1.jar");
    }

    #[test]
    fn maven_classifier_resolves() {
        let l = Library {
            name: "org.lwjgl:lwjgl:3.3.1:natives-windows".into(),
            downloads: Downloads::default(),
            rules: vec![],
            natives: HashMap::new(),
            extract: None,
            url: None,
            sha1: None,
            size: None,
        };
        assert_eq!(
            l.maven_path().unwrap(),
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar"
        );
    }

    #[test]
    fn classpath_separator_is_os_correct() {
        assert_eq!(Os::Windows.cp_sep(), ";");
        assert_eq!(Os::Linux.cp_sep(), ":");
    }

    #[test]
    fn loader_libraries_precede_vanilla() {
        let child = VersionJson {
            id: "fabric-1.20.4".into(),
            main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
            libraries: vec![Library {
                name: "net.fabricmc:fabric-loader:0.15".into(),
                downloads: Downloads::default(),
                rules: vec![],
                natives: HashMap::new(),
                extract: None,
                url: None,
                sha1: None,
                size: None,
            }],
            arguments: Arguments::default(),
            minecraft_arguments: None,
            asset_index: None,
            assets: None,
            downloads: HashMap::new(),
            java_version: None,
            inherits_from: Some("1.20.4".into()),
        };
        let parent = VersionJson {
            id: "1.20.4".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: vec![Library {
                name: "com.mojang:patchy:1.1".into(),
                downloads: Downloads::default(),
                rules: vec![],
                natives: HashMap::new(),
                extract: None,
                url: None,
                sha1: None,
                size: None,
            }],
            arguments: Arguments::default(),
            minecraft_arguments: None,
            asset_index: None,
            assets: None,
            downloads: HashMap::new(),
            java_version: Some(JavaVersion { major_version: 17 }),
            inherits_from: None,
        };
        let m = VersionJson::inherit(child, parent);
        assert!(m.libraries[0].name.contains("fabric-loader"), "loader must shadow vanilla");
        assert_eq!(m.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(m.required_java(), 17, "java version must be inherited");
    }

    #[test]
    fn java_defaults_to_8_pre_117() {
        let v = VersionJson {
            id: "1.8.9".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: vec![],
            arguments: Arguments::default(),
            minecraft_arguments: Some("--username ${auth_player_name}".into()),
            asset_index: None,
            assets: None,
            downloads: HashMap::new(),
            java_version: None,
            inherits_from: None,
        };
        assert_eq!(v.required_java(), 8);
    }
}
