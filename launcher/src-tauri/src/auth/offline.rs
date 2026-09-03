//! Offline profiles — launch the game with nothing but a username.
//!
//! # What changed and why
//!
//! Until v2.12.0 this client refused every launch without a real Microsoft
//! session (see README "Accounts" and ARSEX_SPEC — that was the project's own
//! standing commitment). On 2026-09-03 the project owner directed the
//! reversal: an explicit, username-only offline launch belongs in the
//! settings' demo section. Offline profiles are a standard launcher feature
//! (Prism, PolyMC, MultiMC all ship one), so this module provides it —
//! honestly labelled, and without touching the Microsoft path, which remains
//! the default when no offline profile is set.
//!
//! # What it is, precisely
//!
//!   * The identity is `{ name, uuid }` with the STANDARD offline UUID
//!     scheme — UUID v3 (MD5) of `"OfflinePlayer:<name>"`, bit-for-bit what
//!     a vanilla offline-mode server assigns. Worlds, LAN play and player
//!     data therefore behave exactly as they would under any other offline
//!     launcher.
//!   * There is no session and no token, because there is nothing to
//!     authenticate against: `launch_game` passes an empty access token and
//!     `--userType legacy`.
//!   * Consequence, stated plainly: singleplayer and open-to-LAN work;
//!     ONLINE-MODE SERVERS WILL REJECT THE JOIN. Mojang's session servers
//!     validate the token the client presents, and no launcher can forge
//!     that. The settings card says the same thing.
//!   * The profile lives in memory (AppState), not on disk as an account —
//!     it can never masquerade as a real sign-in later.

use serde::Serialize;

/// An explicit offline identity. No credential fields exist by construction.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OfflineProfile {
    pub name: String,
    pub uuid: String,
}

/// The standard offline UUID: v3 (MD5) of "OfflinePlayer:<name>" with the
/// RFC version/variant bits forced, identical to Java's
/// `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes())`.
pub fn offline_uuid(name: &str) -> String {
    let d = md5::compute(format!("OfflinePlayer:{name}").as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&d.0);
    b[6] = (b[6] & 0x0f) | 0x30; // version 3
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// Validate and build an offline profile. Minecraft usernames: 1-16 chars,
/// ASCII letters, digits, underscore — the same rule the real game enforces.
pub fn offline_profile(name: &str) -> Result<OfflineProfile, String> {
    let n = name.trim();
    if n.is_empty() || n.len() > 16 {
        return Err("username must be 1-16 characters".into());
    }
    if !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("username may only contain letters, numbers and underscore".into());
    }
    Ok(OfflineProfile {
        name: n.to_string(),
        uuid: offline_uuid(n),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_match_the_vanilla_offline_scheme() {
        // Cross-checked against Java UUID.nameUUIDFromBytes byte-for-byte.
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(offline_uuid("Shadow"), "4de410d7-18a1-3a45-b859-49d4164a8f5c");
        assert_eq!(offline_uuid("Tester_01"), "e09c0022-0be8-3d76-83d8-14c863ad479f");
    }

    #[test]
    fn stable_deterministic_v3_shape() {
        let a = offline_profile("Kagemitsu").unwrap();
        let b = offline_profile("Kagemitsu").unwrap();
        assert_eq!(a, b, "same name must give the same profile");
        assert_eq!(a.uuid.chars().nth(14), Some('3'), "must be a v3 UUID");
        let c = offline_profile("Other").unwrap();
        assert_ne!(a.uuid, c.uuid);
    }

    #[test]
    fn username_rules_match_the_game() {
        assert!(offline_profile("x").is_ok());
        assert!(offline_profile("A_09_zZ").is_ok());
        assert!(offline_profile("").is_err());
        assert!(offline_profile("  ").is_err());
        assert!(offline_profile("way_too_long_username").is_err());
        assert!(offline_profile("bad name").is_err());
        assert!(offline_profile("bad;drop").is_err());
    }

    #[test]
    fn profile_carries_no_credential() {
        let j = serde_json::to_string(&offline_profile("Tester").unwrap()).unwrap();
        assert!(!j.contains("token"), "an offline profile has no token field");
    }
}
