//! Arsex launch engine.
//!
//! Deliberately free of any GUI dependency so it builds and tests anywhere,
//! including CI without a desktop stack. `arsex` (the Tauri crate) depends on
//! this; this depends on nothing of `arsex`.

pub mod args;
pub mod install;
pub mod manifest;
pub mod mods;

pub use manifest::{Os, VersionJson};
pub use mods::{Loader, ModInfo};
