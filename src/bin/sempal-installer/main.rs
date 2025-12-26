#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
use std::env;

mod cleanup;
mod download;
mod install;
mod paths;
mod registry;
mod shortcuts;
mod ui;

const APP_NAME: &str = "SemPal";
const APP_PUBLISHER: &str = "SemPal";
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SemPal";

fn main() -> eframe::Result<()> {
    if env::args().any(|arg| arg == "--uninstall") {
        if let Err(err) = cleanup::run_uninstall() {
            eprintln!("Uninstall failed: {err}");
        }
        return Ok(());
    }

    if env::args().any(|arg| arg == "--dry-run") {
        if let Err(err) = install::run_dry_run() {
            eprintln!("Dry run failed: {err}");
        }
        return Ok(());
    }

    ui::run_installer_app()
}

#[cfg(test)]
mod tests {
    use super::install::{plan_install, PlanAction};
    use std::fs;

    #[test]
    fn dry_run_plans_bundle_copies() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bundle = temp.path().join("bundle");
        let install = temp.path().join("install");
        fs::create_dir_all(bundle.join("bin")).expect("bundle dir");
        fs::write(bundle.join("bin").join("sempal.exe"), "test").expect("exe");

        let plan = plan_install(&bundle, &install).expect("plan");
        let copies = plan
            .actions
            .iter()
            .filter(|action| matches!(action, PlanAction::Copy { .. }))
            .count();
        assert_eq!(copies, 1);
    }
}
