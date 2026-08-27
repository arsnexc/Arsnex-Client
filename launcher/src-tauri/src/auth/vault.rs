//! DPAPI-backed token vault.
//!
//! Refresh tokens are long-lived and are the single most valuable secret Arsex
//! holds. We seal them with `CryptProtectData` at CURRENT_USER scope with an
//! additional per-install entropy blob.
//!
//! Practical consequence: copying `vault.dat` to another machine, or opening it
//! as a different Windows user, yields nothing. The ciphertext is bound to the
//! user's logon credential.
//!
//! We deliberately do NOT use Windows Credential Manager for this. WCM entries
//! are enumerable by any process running as the user and are readable in bulk by
//! common credential-dumping tooling. DPAPI + a file we control is a smaller
//! target and lets us add our own entropy.

use anyhow::{anyhow, Result};
use zeroize::Zeroizing;

#[cfg(windows)]
mod sys {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN,
        CRYPT_INTEGER_BLOB,
    };

    pub struct Blob(pub Vec<u8>);

    fn to_blob(v: &[u8]) -> CRYPTOAPI_BLOB {
        CRYPTOAPI_BLOB {
            cbData: v.len() as u32,
            pbData: v.as_ptr() as *mut u8,
        }
    }

    unsafe fn take(out: CRYPTOAPI_BLOB) -> Vec<u8> {
        let slice = std::slice::from_raw_parts(out.pbData, out.cbData as usize);
        let owned = slice.to_vec();
        let _ = LocalFree(HLOCAL(out.pbData as _));
        owned
    }

    pub fn protect(plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let mut out = CRYPTOAPI_BLOB::default();
            let inb = to_blob(plain);
            let ent = to_blob(entropy);
            CryptProtectData(
                &inb,
                windows::core::w!("Arsex Client token"),
                Some(&ent),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| format!("CryptProtectData: {e}"))?;
            Ok(take(out))
        }
    }

    pub fn unprotect(cipher: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let mut out = CRYPTOAPI_BLOB::default();
            let inb = to_blob(cipher);
            let ent = to_blob(entropy);
            CryptUnprotectData(
                &inb,
                None,
                Some(&ent),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| format!("CryptUnprotectData: {e}"))?;
            Ok(take(out))
        }
    }
}

/// Per-install entropy. Generated once, stored beside the vault.
/// This means a stolen vault.dat ALSO needs the entropy file, and both still
/// need the original Windows user account. Defence in depth.
fn entropy() -> Result<Vec<u8>> {
    use rand::RngCore;
    let path = crate::paths::data_dir()?.join("vault.entropy");
    if let Ok(existing) = std::fs::read(&path) {
        if existing.len() == 32 {
            return Ok(existing);
        }
    }
    let mut e = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut e);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, &e)?;

    #[cfg(windows)]
    {
        // Hide it. Not security, just keeps it out of the way.
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        let _ = std::fs::OpenOptions::new()
            .write(true)
            .attributes(FILE_ATTRIBUTE_HIDDEN)
            .open(&path);
    }
    Ok(e)
}

pub fn seal(plaintext: &str) -> Result<Vec<u8>> {
    let ent = entropy()?;
    #[cfg(windows)]
    {
        sys::protect(plaintext.as_bytes(), &ent).map_err(|e| anyhow!(e))
    }
    #[cfg(not(windows))]
    {
        // Dev-only fallback so the crate builds on Linux/macOS CI.
        // Never shipped: release builds are windows-only targets.
        let _ = ent;
        Err(anyhow!("token vault requires Windows (DPAPI)"))
    }
}

pub fn unseal(cipher: &[u8]) -> Result<Zeroizing<String>> {
    let ent = entropy()?;
    #[cfg(windows)]
    {
        let plain = sys::unprotect(cipher, &ent).map_err(|e| anyhow!(e))?;
        let s = String::from_utf8(plain)?;
        Ok(Zeroizing::new(s))
    }
    #[cfg(not(windows))]
    {
        let _ = (cipher, ent);
        Err(anyhow!("token vault requires Windows (DPAPI)"))
    }
}

/// Wipe every stored credential. Called by "Sign out of all accounts".
pub fn purge() -> Result<()> {
    let dir = crate::paths::data_dir()?;
    for f in ["vault.dat", "vault.entropy"] {
        let p = dir.join(f);
        if p.exists() {
            // Overwrite before unlink. SSD wear-levelling makes this imperfect,
            // but it defeats trivial undelete.
            if let Ok(meta) = std::fs::metadata(&p) {
                let _ = std::fs::write(&p, vec![0u8; meta.len() as usize]);
            }
            std::fs::remove_file(&p)?;
        }
    }
    Ok(())
}
