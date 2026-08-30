fn main() {
    tauri_build::build();

    // Declared so rustc knows this cfg is expected (see below); without it
    // every CI build warns about an "unexpected cfg condition".
    println!("cargo::rustc-check-cfg=cfg(arsex_mod_bundled)");

    // The Arsex mod jar is embedded into the launcher at compile time so a
    // fresh install can provision a Fabric instance with zero network beyond
    // what a launch already needs. CI copies the CI-built jar to
    // launcher/src-tauri/resources/arsex-mod.jar before `cargo tauri build`.
    // Local checkouts won't have it — the cfg keeps them compiling, and at
    // runtime the launcher says plainly that this build carries no mod.
    // See game/bundled.rs.
    let jar = std::path::Path::new("resources/arsex-mod.jar");
    let bundled = std::fs::metadata(jar).map(|m| m.len() > 0).unwrap_or(false);
    if bundled {
        println!("cargo:rerun-if-changed=resources/arsex-mod.jar");
        println!("cargo:rustc-cfg=arsex_mod_bundled");
        println!("cargo:rustc-env=ARSEX_MOD_BUNDLED=1");
    } else {
        println!("cargo:rustc-env=ARSEX_MOD_BUNDLED=0");
    }
}
