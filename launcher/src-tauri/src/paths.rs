//! Filesystem layout.
//!
//! Every writable path the client uses resolves through here. Nothing writes
//! next to the executable: an NSIS `currentUser` install can land in
//! `%LOCALAPPDATA%\Programs`, and a per-machine install lands in Program Files
//! where a standard user has no write access at all.

use anyhow::{anyhow, Result};
use std::{fs, path::PathBuf};

/// `%APPDATA%\Arsex` — config, tokens, profiles. Roams with the user.
pub fn data_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().ok_or_else(|| anyhow!("no home directory"))?;
    let p = base.data_dir().join("Arsex");
    fs::create_dir_all(&p)?;
    Ok(p)
}

/// `%LOCALAPPDATA%\Arsex` — caches, libraries, assets. Deliberately non-roaming:
/// a Minecraft asset cache is gigabytes and must never hit a roaming profile.
pub fn cache_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().ok_or_else(|| anyhow!("no home directory"))?;
    let p = base.data_local_dir().join("Arsex");
    fs::create_dir_all(&p)?;
    Ok(p)
}

pub fn log_dir() -> Result<PathBuf> {
    let p = cache_dir()?.join("logs");
    fs::create_dir_all(&p)?;
    Ok(p)
}

pub fn crash_dir() -> Result<PathBuf> {
    let p = cache_dir()?.join("crashes");
    fs::create_dir_all(&p)?;
    Ok(p)
}

/// Root for a named instance: `%LOCALAPPDATA%\Arsex\instances\<slug>`.
pub fn instance_dir(slug: &str) -> Result<PathBuf> {
    if slug.is_empty()
        || slug.len() > 64
        || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        // Guards against `..\..\Windows\System32` arriving from the wizard.
        return Err(anyhow!("invalid instance slug: {slug:?}"));
    }
    let p = cache_dir()?.join("instances").join(slug);
    fs::create_dir_all(&p)?;
    Ok(p)
}

pub fn assets_dir() -> Result<PathBuf> {
    let p = cache_dir()?.join("assets");
    fs::create_dir_all(&p)?;
    Ok(p)
}

pub fn libraries_dir() -> Result<PathBuf> {
    let p = cache_dir()?.join("libraries");
    fs::create_dir_all(&p)?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(instance_dir("..").is_err());
        assert!(instance_dir("../../windows").is_err());
        assert!(instance_dir("a\\b").is_err());
        assert!(instance_dir("").is_err());
    }

    #[test]
    fn accepts_slugs() {
        assert!(instance_dir("ranked-duels").is_ok());
        assert!(instance_dir("main_2").is_ok());
    }
}
