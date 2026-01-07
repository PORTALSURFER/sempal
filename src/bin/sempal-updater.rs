//! Standalone updater helper used to apply updates on Windows.
//!
//! The main `sempal` app can spawn this executable and exit so that the helper can
//! safely replace the installed binaries.

#[path = "sempal-updater/ui.rs"]
mod ui;

use std::path::PathBuf;

use sempal::updater::{
    APP_NAME, REPO_SLUG, RuntimeIdentity, UpdateChannel, UpdaterRunArgs, apply_update,
};

fn main() {
    if let Err(err) = try_main() {
        eprintln!("Update failed: {err}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<(), String> {
    let (args, headless) = parse_args(std::env::args().skip(1).collect())?;
    if headless {
        return run_headless(args);
    }
    ui::run_gui(args)
}

fn run_headless(args: UpdaterRunArgs) -> Result<(), String> {
    let plan = apply_update(args).map_err(|err| err.to_string())?;
    eprintln!(
        "Updated {} from {} into {}",
        APP_NAME,
        plan.release_tag,
        plan.install_dir.display()
    );
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<(UpdaterRunArgs, bool), String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Err(help_text());
    }
    let mut state = ArgState::new()?;
    let mut i = 0;
    while i < args.len() {
        state.apply_arg(&args, &mut i)?;
        i += 1;
    }
    state.finish()
}

struct ArgState {
    repo: String,
    channel: UpdateChannel,
    install_dir: Option<PathBuf>,
    relaunch: bool,
    target: String,
    platform: String,
    arch: String,
    requested_tag: Option<String>,
    headless: bool,
}

impl ArgState {
    fn new() -> Result<Self, String> {
        Ok(Self {
            repo: REPO_SLUG.to_string(),
            channel: UpdateChannel::Stable,
            install_dir: None,
            relaunch: true,
            target: default_target().ok_or_else(|| "Unsupported target".to_string())?,
            platform: default_platform().ok_or_else(|| "Unsupported platform".to_string())?,
            arch: default_arch().ok_or_else(|| "Unsupported arch".to_string())?,
            requested_tag: None,
            headless: false,
        })
    }

    fn apply_arg(&mut self, args: &[String], i: &mut usize) -> Result<(), String> {
        let arg = &args[*i];
        match arg.as_str() {
            "--repo" => {
                self.repo = next_value(args, i, "--repo")?;
            }
            "--channel" => {
                let value = next_value(args, i, "--channel")?;
                self.channel = match value.as_str() {
                    "stable" => UpdateChannel::Stable,
                    "nightly" => UpdateChannel::Nightly,
                    other => return Err(format!("Unknown channel '{other}'")),
                };
            }
            "--install-dir" => {
                self.install_dir = Some(PathBuf::from(next_value(args, i, "--install-dir")?));
            }
            "--no-relaunch" => {
                self.relaunch = false;
            }
            "--tag" => {
                self.requested_tag = Some(next_value(args, i, "--tag")?);
            }
            "--headless" => {
                self.headless = true;
            }
            "--target" => {
                self.target = next_value(args, i, "--target")?;
            }
            "--platform" => {
                self.platform = next_value(args, i, "--platform")?;
            }
            "--arch" => {
                self.arch = next_value(args, i, "--arch")?;
            }
            unknown => return Err(format!("Unknown argument '{unknown}'\n\n{}", help_text())),
        }
        Ok(())
    }

    fn finish(self) -> Result<(UpdaterRunArgs, bool), String> {
        let install_dir = self
            .install_dir
            .ok_or_else(|| format!("Missing --install-dir\n\n{}", help_text()))?;
        Ok((
            UpdaterRunArgs {
                repo: self.repo,
                identity: RuntimeIdentity {
                    app: APP_NAME.to_string(),
                    channel: self.channel,
                    target: self.target,
                    platform: self.platform,
                    arch: self.arch,
                },
                install_dir,
                relaunch: self.relaunch,
                requested_tag: self.requested_tag,
            },
            self.headless,
        ))
    }
}

fn next_value(args: &[String], i: &mut usize, name: &str) -> Result<String, String> {
    let next = args
        .get(*i + 1)
        .ok_or_else(|| format!("Missing value for {name}"))?;
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
  --tag <TAG>                  Install a specific release tag\n\
  --no-relaunch                Do not relaunch the app after update\n\
  --headless                   Run without GUI output\n\
  -h, --help                   Show help\n"
    )
}

fn default_target() -> Option<String> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some("x86_64-pc-windows-msvc".to_string());
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return Some("x86_64-unknown-linux-gnu".to_string());
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return Some("aarch64-unknown-linux-gnu".to_string());
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Some("x86_64-apple-darwin".to_string());
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Some("aarch64-apple-darwin".to_string());
    }
    #[allow(unreachable_code)]
    None
}

fn default_platform() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        return Some("windows".to_string());
    }
    #[cfg(target_os = "linux")]
    {
        return Some("linux".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        return Some("macos".to_string());
    }
    #[allow(unreachable_code)]
    None
}

fn default_arch() -> Option<String> {
    #[cfg(target_arch = "x86_64")]
    {
        return Some("x86_64".to_string());
    }
    #[cfg(target_arch = "aarch64")]
    {
        return Some("aarch64".to_string());
    }
    #[allow(unreachable_code)]
    None
}
