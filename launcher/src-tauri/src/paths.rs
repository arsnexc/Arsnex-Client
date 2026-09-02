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

/// Guard for opening links from the webview in the system browser. The news
/// card carries release URLs; only exact-host https links to the project's
/// own GitHub presence may ever reach `open::that`. Everything else — http,
/// other hosts, scheme or host tricks — is refused.
pub fn is_safe_external_url(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    u.scheme() == "https"
        && matches!(u.host_str(), Some("github.com") | Some("www.github.com"))
        && u.path().starts_with("/arsnexc/")
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
    fn only_project_github_https_links_pass() {
        use super::is_safe_external_url as safe;
        assert!(safe("https://github.com/arsnexc/Arsnex-Client/releases/tag/v2.9.0"));
        assert!(!safe("http://github.com/arsnexc/Arsnex-Client"));
        assert!(!safe("https://evil.com/github.com/arsnexc"));
        assert!(!safe("https://github.com/other/repo"));
        assert!(!safe("file:///C:/Windows/System32/cmd.exe"));
        assert!(!safe("https://github.com.evil.io/arsnexc/x"));
        assert!(!safe("javascript:alert(1)"));
        assert!(!safe("not a url"));
    }

    #[test]
    fn accepts_slugs() {
        assert!(instance_dir("ranked-duels").is_ok());
        assert!(instance_dir("main_2").is_ok());
    }
}
