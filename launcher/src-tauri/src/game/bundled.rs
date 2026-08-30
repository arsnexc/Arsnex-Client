//! The Arsex Fabric mod, embedded in the launcher.
//!
//! Why embed rather than download at runtime: the jar is ~45 KB, the launcher
//! already requires network for the Mojang manifest on every launch, and a
//! pinned copy in the binary means the modules can never be severed from the
//! launcher by a moved release URL. Integrity is the compiler's problem, not
//! the network's.
//!
//! CI builds the mod first (`mod` job), copies the jar here as
//! `resources/arsex-mod.jar`, then builds the launcher — see build.rs, which
//! sets `arsex_mod_bundled` only when a non-empty jar is present. A local
//! `cargo run` without the jar compiles fine and reports the gap honestly.

/// Must match `mod_version` in `mod/gradle.properties` — the version embedded
/// in this build. Installed as `arsex-mod-<version>.jar` in the instance.
pub const ARSEX_MOD_VERSION: &str = "2.5.0";

/// The jar's bytes, present only in CI-built release binaries.
#[cfg(arsex_mod_bundled)]
pub static ARSEX_MOD_JAR: &[u8] = include_bytes!("../../resources/arsex-mod.jar");

/// The bytes to install, or None in builds that were not given a jar.
pub fn jar_bytes() -> Option<&'static [u8]> {
    #[cfg(arsex_mod_bundled)]
    {
        Some(ARSEX_MOD_JAR)
    }
    #[cfg(not(arsex_mod_bundled))]
    {
        None
    }
}

/// File name the jar is installed under inside an instance's mods directory.
pub fn jar_file_name() -> String {
    format!("arsex-mod-{ARSEX_MOD_VERSION}.jar")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_carries_the_version() {
        assert!(jar_file_name().starts_with("arsex-mod-"));
        assert!(jar_file_name().ends_with(".jar"));
    }
}
