//! Standalone updater helper used to apply updates on Windows.
//!
//! The main `sempal` app can spawn this executable and exit so that the helper can
//! safely replace the installed binaries.

use std::path::PathBuf;

use sempal::updater::{RuntimeIdentity, UpdateChannel, UpdaterRunArgs, apply_update, APP_NAME, REPO_SLUG};

fn main() {
    if let Err(err) = try_main() {
        eprintln!("Update failed: {err}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1).collect())?;
    let plan = apply_update(args).map_err(|err| err.to_string())?;
    eprintln!(
        "Updated {} from {} into {}",
        APP_NAME,
        plan.release_tag,
        plan.install_dir.display()
    );
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<UpdaterRunArgs, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Err(help_text());
    }
    let mut repo = REPO_SLUG.to_string();
    let mut channel = UpdateChannel::Stable;
    let mut install_dir: Option<PathBuf> = None;
    let mut relaunch = true;
    let mut target = default_target().ok_or_else(|| "Unsupported target".to_string())?;
    let mut platform = default_platform().ok_or_else(|| "Unsupported platform".to_string())?;
    let mut arch = default_arch().ok_or_else(|| "Unsupported arch".to_string())?;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--repo" => {
                repo = next_value(&args, &mut i, "--repo")?;
            }
            "--channel" => {
                let value = next_value(&args, &mut i, "--channel")?;
                channel = match value.as_str() {
                    "stable" => UpdateChannel::Stable,
                    "nightly" => UpdateChannel::Nightly,
                    other => return Err(format!("Unknown channel '{other}'")),
                };
            }
            "--install-dir" => {
                install_dir = Some(PathBuf::from(next_value(&args, &mut i, "--install-dir")?));
            }
            "--no-relaunch" => {
                relaunch = false;
            }
            "--target" => {
                target = next_value(&args, &mut i, "--target")?;
            }
            "--platform" => {
                platform = next_value(&args, &mut i, "--platform")?;
            }
            "--arch" => {
                arch = next_value(&args, &mut i, "--arch")?;
            }
            unknown => return Err(format!("Unknown argument '{unknown}'\n\n{}", help_text())),
        }
        i += 1;
    }

    let install_dir = install_dir.ok_or_else(|| format!("Missing --install-dir\n\n{}", help_text()))?;
    Ok(UpdaterRunArgs {
        repo,
        identity: RuntimeIdentity {
            app: APP_NAME.to_string(),
            channel,
            target,
            platform,
            arch,
        },
        install_dir,
        relaunch,
    })
}

fn next_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    let next = args.get(*i + 1).ok_or_else(|| format!("Missing value for {name}"))?;
    *i += 1;
    Ok(next.clone())
}

fn help_text() -> String {
    format!(
        "Usage: {APP_NAME}-updater --install-dir <dir> [options]\n\n\
Options:\n\
  --channel <stable|nightly>   Update channel (default: stable)\n\
  --repo <OWNER/REPO>          GitHub repository (default: {REPO_SLUG})\n\
  --target <TRIPLE>            Target triple (default: detected)\n\
  --platform <LABEL>           Platform label (default: detected)\n\
  --arch <LABEL>               Arch label (default: detected)\n\
  --no-relaunch                Do not relaunch the app after update\n\
  -h, --help                   Show help\n"
    )
}

fn default_target() -> Option<String> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some("x86_64-pc-windows-msvc".to_string());
    }
    #[allow(unreachable_code)]
    None
}

fn default_platform() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return Some("windows".to_string());
    }
    #[allow(unreachable_code)]
    None
}

fn default_arch() -> Option<String> {
    #[cfg(target_arch = "x86_64")]
    {
        return Some("x86_64".to_string());
    }
    #[allow(unreachable_code)]
    None
}

