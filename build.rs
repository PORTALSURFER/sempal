use std::{env, fs, io, path::Path, time::Duration};

/// Shared version helpers used by the build script.
mod version_tools {
    include!("build/version_tools.rs");
}

const MIN_BUMP_SPACING: Duration = Duration::from_secs(300);

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build/version_tools.rs");
    if env::var("SKIP_VERSION_BUMP").is_ok() {
        println!("cargo:warning=Skipping version bump because SKIP_VERSION_BUMP is set");
        return;
    }
    let force_bump = env::var("FORCE_VERSION_BUMP").is_ok();
    if let Err(error) = bump_manifest_version(force_bump) {
        eprintln!("Version bump failed: {error}");
        std::process::exit(1);
    }
}

fn bump_manifest_version(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = Path::new("Cargo.toml");
    let manifest = fs::read_to_string(manifest_path)?;
    let lock_path = Path::new("target").join("version.bump.lock");
    if !force {
        if let Ok(meta) = fs::metadata(&lock_path) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().unwrap_or_default() < MIN_BUMP_SPACING {
                    println!(
                        "cargo:warning=Skipping version bump because a recent bump is already recorded"
                    );
                    return Ok(());
                }
            }
        }
    }
    let mut doc = manifest
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid manifest: {error}"),
            )
        })?;
    let current = doc["package"]["version"]
        .as_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "package.version not found"))?;
    let next = version_tools::bump_minor(current)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if next == current {
        return Ok(());
    }
    doc["package"]["version"] = toml_edit::value(next.clone());
    fs::write(manifest_path, doc.to_string())?;
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&lock_path, &next)?;
    let stamp_path = Path::new("target").join("version.bumped");
    if let Some(parent) = stamp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&stamp_path, &next)?;
    println!(
        "cargo:warning=Version bumped to {next} (stamp: {})",
        stamp_path.display()
    );
    Ok(())
}
