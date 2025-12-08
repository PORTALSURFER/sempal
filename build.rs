use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Shared version helpers used by the build script.
mod version_tools {
    include!("build/version_tools.rs");
}

const MIN_BUMP_SPACING: Duration = Duration::from_secs(300);
const PACKAGE_NAME: &str = "sempal";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build/version_tools.rs");
    println!("cargo:rerun-if-changed=build/windows/sempal.rc");
    println!("cargo:rerun-if-changed=assets/logo3.ico");

    if compiling_for_windows_target() {
        if let Err(error) = compile_windows_resources() {
            eprintln!("Failed to embed Windows resources: {error}");
            std::process::exit(1);
        }
    }
    if env::var("SKIP_VERSION_BUMP").is_ok() {
        println!("cargo:warning=Skipping version bump because SKIP_VERSION_BUMP is set");
        return;
    }
    let force_bump = env::var("FORCE_VERSION_BUMP").is_ok();
    if let Err(error) = try_main(force_bump) {
        eprintln!("Version bump failed: {error}");
        std::process::exit(1);
    }
}

fn try_main(force_bump: bool) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = Path::new("Cargo.toml");
    let target_dir = env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let lock_path = target_dir.join("version.bump.lock");
    let stamp_path = target_dir.join("version.bumped");
    let last_success = latest_successful_build(&target_dir, &profile)?;
    let last_bump = modified_time(&lock_path)?;
    if let Some(reason) = skip_bump_reason(force_bump, last_success, last_bump)? {
        println!("cargo:warning={reason}");
        return Ok(());
    }
    let next = bump_manifest_version(manifest_path)?;
    record_bump(&lock_path, &stamp_path, &next)?;
    println!(
        "cargo:warning=Version bumped to {next} (stamp: {})",
        stamp_path.display()
    );
    Ok(())
}

// Use Cargo fingerprints as a proxy for the most recent successful build.
fn latest_successful_build(target_dir: &Path, profile: &str) -> io::Result<Option<SystemTime>> {
    let fingerprints = target_dir.join(profile).join(".fingerprint");
    let entries = match fs::read_dir(&fingerprints) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut newest = None;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if !entry
            .file_name()
            .to_str()
            .map(|name| name.starts_with(PACKAGE_NAME))
            .unwrap_or(false)
        {
            continue;
        }
        let candidate = latest_mod_time(entry.path())?;
        newest = newer(newest, candidate);
    }
    Ok(newest)
}

fn latest_mod_time(path: PathBuf) -> io::Result<Option<SystemTime>> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut newest = None;
    for entry in entries {
        let entry = entry?;
        let modified = entry.metadata()?.modified()?;
        newest = newer(newest, Some(modified));
    }
    Ok(newest)
}

fn modified_time(path: &Path) -> io::Result<Option<SystemTime>> {
    match fs::metadata(path) {
        Ok(meta) => Ok(Some(meta.modified()?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn skip_bump_reason(
    force_bump: bool,
    last_success: Option<SystemTime>,
    last_bump: Option<SystemTime>,
) -> io::Result<Option<String>> {
    if force_bump {
        return Ok(None);
    }
    let Some(success_time) = last_success else {
        return Ok(Some(
            "Skipping version bump because no successful build is recorded yet".into(),
        ));
    };
    if let Some(bump_time) = last_bump {
        if success_time <= bump_time {
            return Ok(Some(
                "Skipping version bump because the last bump already followed the most recent successful build"
                    .into(),
            ));
        }
        if SystemTime::now()
            .duration_since(bump_time)
            .unwrap_or_default()
            < MIN_BUMP_SPACING
        {
            return Ok(Some(
                "Skipping version bump because a recent bump is already recorded".into(),
            ));
        }
    }
    Ok(None)
}

fn bump_manifest_version(manifest_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let manifest = fs::read_to_string(manifest_path)?;
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
        return Ok(next);
    }
    doc["package"]["version"] = toml_edit::value(next.clone());
    fs::write(manifest_path, doc.to_string())?;
    Ok(next)
}

fn record_bump(lock_path: &Path, stamp_path: &Path, next: &str) -> io::Result<()> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(lock_path, next)?;
    if let Some(parent) = stamp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(stamp_path, next)?;
    Ok(())
}

fn newer(current: Option<SystemTime>, candidate: Option<SystemTime>) -> Option<SystemTime> {
    match (current, candidate) {
        (None, time) | (time, None) => time,
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
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
