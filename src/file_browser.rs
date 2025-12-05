use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use slint::SharedString;
use sysinfo::Disks;

/// A single file or directory entry.
#[derive(Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Describes how the browser should respond when an entry is activated.
pub enum EntryAction {
    OpenDir,
    PlayFile(PathBuf),
    None,
}

/// Minimal filesystem browser for selecting wav files.
pub struct FileBrowser {
    mounts: Vec<PathBuf>,
    current_dir: PathBuf,
    last_click: Option<(PathBuf, Instant)>,
}

impl FileBrowser {
    /// Initialize the browser with detected mount points.
    pub fn new() -> Self {
        let mounts = Self::collect_mounts();
        let current_dir = mounts
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/"));
        Self {
            mounts,
            current_dir,
            last_click: None,
        }
    }

    /// Return the list of mount points suitable for UI presentation.
    pub fn mounts(&self) -> Vec<SharedString> {
        self.mounts
            .iter()
            .map(|p| SharedString::from(p.display().to_string()))
            .collect()
    }

    /// Index of the currently selected disk for UI binding.
    pub fn selected_disk(&self) -> i32 {
        self.mounts
            .iter()
            .position(|p| self.current_dir.starts_with(p))
            .unwrap_or(0) as i32
    }

    /// Switch to a given disk by index.
    pub fn select_disk(&mut self, index: usize) {
        if let Some(path) = self.mounts.get(index) {
            self.current_dir = path.clone();
            self.last_click = None;
        }
    }

    /// Navigate to the parent directory if possible.
    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
        }
        self.last_click = None;
    }

    /// List wav files and directories in the current directory.
    pub fn entries(&self) -> Vec<FileEntry> {
        let mut items = Vec::new();
        if let Ok(read_dir) = fs::read_dir(&self.current_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if !is_dir && !Self::is_wav(&path) {
                    continue;
                }
                let name = entry
                    .file_name()
                    .to_string_lossy()
                    .to_string()
                    .trim()
                    .to_string();
                items.push(FileEntry { name, path, is_dir });
            }
        }
        items.sort_by(Self::compare_entries);
        items
    }

    /// Handle a click on an entry, returning the resulting action.
    pub fn activate_entry(&mut self, entry: &FileEntry) -> EntryAction {
        if entry.is_dir {
            let is_double = self.is_double_click(&entry.path);
            self.last_click = Some((entry.path.clone(), Instant::now()));
            if is_double {
                self.current_dir = entry.path.clone();
                EntryAction::OpenDir
            } else {
                EntryAction::None
            }
        } else {
            self.last_click = None;
            EntryAction::PlayFile(entry.path.clone())
        }
    }

    fn is_double_click(&self, path: &Path) -> bool {
        self.last_click
            .as_ref()
            .map(|(prev, time)| prev == path && time.elapsed().as_millis() < 500)
            .unwrap_or(false)
    }

    fn collect_mounts() -> Vec<PathBuf> {
        let mut mounts: Vec<PathBuf> = Disks::new_with_refreshed_list()
            .iter()
            .map(|d| d.mount_point().to_path_buf())
            .collect();
        if mounts.is_empty() {
            mounts.push(PathBuf::from("/"));
        }
        mounts.sort();
        mounts.dedup();
        mounts
    }

    fn is_wav(path: &Path) -> bool {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    }

    fn compare_entries(a: &FileEntry, b: &FileEntry) -> Ordering {
        match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    }
}
