//! Build script for platform-specific build configuration.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/windows/sempal.rc");
    println!("cargo:rerun-if-changed=assets/logo3.ico");

    if compiling_for_windows_target()
        && let Err(error) = compile_windows_resources()
    {
        eprintln!("Failed to embed Windows resources: {error}");
        std::process::exit(1);
    }
}

fn compiling_for_windows_target() -> bool {
    env::var("CARGO_CFG_TARGET_OS")
        .map(|target| target == "windows")
        .unwrap_or_else(|_| cfg!(target_os = "windows"))
}

fn compile_windows_resources() -> Result<(), Box<dyn std::error::Error>> {
    embed_resource::compile("build/windows/sempal.rc", embed_resource::NONE).manifest_optional()?;
    Ok(())
}
