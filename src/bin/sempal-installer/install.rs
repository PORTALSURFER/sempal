use std::{collections::HashSet, fs, path::{Path, PathBuf}};

use sempal::app_dirs;

use crate::{download, registry, shortcuts, ui, paths};

pub(crate) struct InstallPlan {
    pub(crate) actions: Vec<PlanAction>,
}

pub(crate) enum PlanAction {
    CreateDir { path: PathBuf },
    Copy { source: PathBuf, target: PathBuf },
}

pub(crate) fn run_install(
    bundle_dir: &Path,
    install_dir: &Path,
    sender: ui::InstallerSender,
) -> Result<(), String> {
    download::ensure_downloads()?;
    let entries = collect_bundle_entries(bundle_dir)?;
    ui::send_started(&sender, entries.len())?;

    fs::create_dir_all(install_dir)
        .map_err(|err| format!("Failed to create install dir: {err}"))?;

    for (idx, (source, relative)) in entries.iter().enumerate() {
        let target = install_dir.join(relative);
        ensure_parent_dir(&target)?;
        fs::copy(source, &target)
            .map_err(|err| format!("Failed to copy {}: {err}", source.display()))?;
        ui::send_file_copied(
            &sender,
            idx + 1,
            relative.display().to_string(),
        )?;
    }

    ensure_app_data_models(bundle_dir)?;
    registry::register_uninstall_entry(install_dir)?;
    shortcuts::create_start_menu_shortcut(install_dir)?;
    ui::send_finished(&sender)?;
    Ok(())
}

pub(crate) fn run_dry_run() -> Result<(), String> {
    let bundle_dir = paths::default_bundle_dir();
    let install_dir = paths::default_install_dir();
    let plan = plan_install(&bundle_dir, &install_dir)?;
    println!(
        "Dry run plan: {} actions for {}",
        plan.actions.len(),
        install_dir.display()
    );
    Ok(())
}

pub(crate) fn plan_install(bundle_dir: &Path, install_dir: &Path) -> Result<InstallPlan, String> {
    let entries = collect_bundle_entries(bundle_dir)?;
    let mut actions = Vec::new();
    let mut seen_dirs = HashSet::new();

    for (source, relative) in entries {
        let target = install_dir.join(relative);
        if let Some(parent) = target.parent() {
            add_dir_action(&mut actions, &mut seen_dirs, parent.to_path_buf());
        }
        actions.push(PlanAction::Copy { source, target });
    }

    let models_dir = app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join("models");
    add_dir_action(&mut actions, &mut seen_dirs, models_dir);

    Ok(InstallPlan { actions })
}

fn add_dir_action(
    actions: &mut Vec<PlanAction>,
    seen_dirs: &mut HashSet<PathBuf>,
    path: PathBuf,
) {
    if seen_dirs.insert(path.clone()) {
        actions.push(PlanAction::CreateDir { path });
    }
}

fn ensure_parent_dir(target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create folder {}: {err}", parent.display()))?;
    }
    Ok(())
}

pub(crate) fn collect_bundle_entries(bundle_dir: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if !bundle_dir.exists() {
        return Err(format!(
            "Bundle directory not found at {}",
            bundle_dir.display()
        ));
    }
    let mut files = Vec::new();
    visit_bundle(bundle_dir, bundle_dir, &mut files)?;
    Ok(files)
}

fn visit_bundle(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|err| format!("Failed to read bundle: {err}"))? {
        let entry = entry.map_err(|err| format!("Failed to read bundle entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            visit_bundle(root, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .map_err(|err| format!("Failed to build relative path: {err}"))?
                .to_path_buf();
            files.push((path, relative));
        }
    }
    Ok(())
}

fn ensure_app_data_models(bundle_dir: &Path) -> Result<(), String> {
    let app_root = app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    let models_dir = app_root.join("models");
    fs::create_dir_all(&models_dir)
        .map_err(|err| format!("Failed to create models directory: {err}"))?;

    let bundle_models = bundle_dir.join("models");
    if bundle_models.exists() {
        for (source, relative) in collect_bundle_entries(&bundle_models)? {
            let target = models_dir.join(relative);
            ensure_parent_dir(&target)?;
            fs::copy(&source, &target)
                .map_err(|err| format!("Failed to copy model {}: {err}", source.display()))?;
        }
    }
    Ok(())
}
