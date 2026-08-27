//! Demo mode — community pre-release testing WITHOUT a Minecraft account.
//!
//! # Why this exists, and what it deliberately is not
//!
//! Testers need to exercise the launcher: navigation, the console, the
//! instance wizard, mod installation, HUD editing, settings persistence,
//! crash recovery. **None of that requires owning Minecraft.** So demo mode
//! unlocks the entire UI surface with a synthetic local profile.
//!
//! What it does NOT do — by construction, not by policy:
//!
//!   * It cannot launch the real game. `can_launch()` returns false and
//!     `main.rs::launch_game` refuses the spawn. There is no argv path that
//!     reaches the JVM from a demo session.
//!   * It never contacts Mojang. No fake session server, no auth interception,
//!     no `--uuid`/`--accessToken` forgery. It cannot join an online server
//!     because it never obtains a session at all.
//!   * It is not persisted as an account. Demo state lives in memory and dies
//!     with the process, so it cannot masquerade as a real login later.
//!
//! That boundary is what separates "test the launcher" from "pirate the game".
//! A cracked launcher forges the session handshake; this one simply declines
//! to have a session. The distinction is enforced at the type level below:
//! `DemoProfile` has no token field for a caller to misuse.

use serde::Serialize;

/// A synthetic identity for UI testing. Note the absence of any credential
/// field — there is nothing here to forge a session with.
#[derive(Debug, Clone, Serialize)]
pub struct DemoProfile {
    pub uuid: String,
    pub username: String,
    /// Always true. The frontend keys its persistent warning banner off this.
    pub demo: bool,
}

/// Deterministic offline UUID so repeated test runs produce stable output in
/// bug reports. Version nibble is forced to 8 (not 4) so this can never be
/// mistaken for a real Mojang v4 UUID by any downstream parser.
fn demo_uuid(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(format!("arsex-demo:{name}").as_bytes());
    let b = &h[..16];
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-8{:01x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6] & 0x0f, b[7],
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

pub fn start(nickname: &str) -> Result<DemoProfile, String> {
    let name = nickname.trim();
    if name.is_empty() || name.len() > 16 {
        return Err("nickname must be 1-16 characters".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("nickname may only contain letters, numbers and underscore".into());
    }
    Ok(DemoProfile {
        uuid: demo_uuid(name),
        username: name.to_string(),
        demo: true,
    })
}

/// The hard gate. Called by `launch_game` before any process spawn.
pub const fn can_launch() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_can_never_launch() {
        assert!(!can_launch(), "demo mode must never be able to spawn the game");
    }

    #[test]
    fn uuid_is_stable_and_not_v4() {
        let a = start("Tester").unwrap();
        let b = start("Tester").unwrap();
        assert_eq!(a.uuid, b.uuid, "same nickname must give a stable uuid");
        // Version nibble sits at index 14 of the hyphenated form.
        let v = a.uuid.chars().nth(14).unwrap();
        assert_eq!(v, '8', "must not look like a real v4 UUID");
    }

    #[test]
    fn distinct_names_distinct_ids() {
        assert_ne!(start("Alpha").unwrap().uuid, start("Beta").unwrap().uuid);
    }

    #[test]
    fn rejects_bad_nicknames() {
        assert!(start("").is_err());
        assert!(start("   ").is_err());
        assert!(start("way_too_long_nickname").is_err());
        assert!(start("bad name").is_err());
        assert!(start("inject;rm -rf").is_err());
    }

    #[test]
    fn profile_carries_no_credential() {
        // Compile-time guarantee: serialising a demo profile can never leak a
        // token, because the struct has nowhere to put one.
        let p = start("Tester").unwrap();
        let j = serde_json::to_string(&p).unwrap();
        assert!(!j.contains("token"), "demo profile must not carry a token");
        assert!(j.contains("\"demo\":true"));
    }
}
