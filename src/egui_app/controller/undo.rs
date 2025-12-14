use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(crate) type UndoResult = Result<(), String>;

pub(crate) struct UndoEntry<T> {
    pub(crate) label: String,
    pub(crate) undo: Box<dyn Fn(&mut T) -> UndoResult>,
    pub(crate) redo: Box<dyn Fn(&mut T) -> UndoResult>,
    _cleanup: Vec<UndoCleanup>,
}

impl<T> UndoEntry<T> {
    pub(crate) fn new(
        label: impl Into<String>,
        undo: impl Fn(&mut T) -> UndoResult + 'static,
        redo: impl Fn(&mut T) -> UndoResult + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            undo: Box::new(undo),
            redo: Box::new(redo),
            _cleanup: Vec::new(),
        }
    }

    pub(crate) fn with_cleanup_dir(mut self, path: PathBuf) -> Self {
        self._cleanup.push(UndoCleanup::dir(path));
        self
    }
}

pub(crate) struct UndoStack<T> {
    undo: VecDeque<UndoEntry<T>>,
    redo: VecDeque<UndoEntry<T>>,
    limit: usize,
}

impl<T> UndoStack<T> {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            limit: limit.max(1),
        }
    }

    pub(crate) fn push(&mut self, entry: UndoEntry<T>) {
        self.redo.clear();
        self.undo.push_back(entry);
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)]
    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn undo(&mut self, target: &mut T) -> Result<Option<String>, String> {
        let Some(entry) = self.undo.pop_back() else {
            return Ok(None);
        };
        let label = entry.label.clone();
        match (entry.undo)(target) {
            Ok(()) => {
                self.redo.push_back(entry);
                Ok(Some(label))
            }
            Err(err) => {
                self.undo.push_back(entry);
                Err(err)
            }
        }
    }

    pub(crate) fn redo(&mut self, target: &mut T) -> Result<Option<String>, String> {
        let Some(entry) = self.redo.pop_back() else {
            return Ok(None);
        };
        let label = entry.label.clone();
        match (entry.redo)(target) {
            Ok(()) => {
                self.undo.push_back(entry);
                Ok(Some(label))
            }
            Err(err) => {
                self.redo.push_back(entry);
                Err(err)
            }
        }
    }
}

struct UndoCleanup {
    dir: Option<PathBuf>,
}

impl UndoCleanup {
    fn dir(dir: PathBuf) -> Self {
        Self { dir: Some(dir) }
    }
}

impl Drop for UndoCleanup {
    fn drop(&mut self) {
        let Some(dir) = self.dir.take() else {
            return;
        };
        let _ = fs::remove_dir_all(dir);
    }
}

pub(crate) struct OverwriteBackup {
    pub(crate) dir: PathBuf,
    pub(crate) before: PathBuf,
    pub(crate) after: PathBuf,
}

impl OverwriteBackup {
    pub(crate) fn capture_before(target: &Path) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("sempal_undo_{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).map_err(|err| format!("Failed to create undo folder: {err}"))?;
        let before = dir.join("before.wav");
        let after = dir.join("after.wav");
        fs::copy(target, &before).map_err(|err| format!("Failed to snapshot audio file: {err}"))?;
        Ok(Self { dir, before, after })
    }

    pub(crate) fn capture_after(&self, target: &Path) -> Result<(), String> {
        fs::copy(target, &self.after)
            .map_err(|err| format!("Failed to snapshot edited audio file: {err}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Counter {
        value: i32,
    }

    #[test]
    fn undo_stack_respects_limit() {
        let mut stack: UndoStack<Counter> = UndoStack::new(3);
        let mut counter = Counter::default();

        for i in 1..=4 {
            counter.value = i;
            let before = i - 1;
            stack.push(UndoEntry::new(
                format!("set {i}"),
                move |c: &mut Counter| {
                    c.value = before;
                    Ok(())
                },
                move |c: &mut Counter| {
                    c.value = i;
                    Ok(())
                },
            ));
        }

        assert_eq!(counter.value, 4);
        assert_eq!(stack.undo(&mut counter).unwrap(), Some("set 4".into()));
        assert_eq!(counter.value, 3);
        assert_eq!(stack.undo(&mut counter).unwrap(), Some("set 3".into()));
        assert_eq!(counter.value, 2);
        assert_eq!(stack.undo(&mut counter).unwrap(), Some("set 2".into()));
        assert_eq!(counter.value, 1);
        assert_eq!(stack.undo(&mut counter).unwrap(), None);
        assert_eq!(counter.value, 1);
    }

    #[test]
    fn pushing_new_action_clears_redo_stack() {
        let mut stack: UndoStack<Counter> = UndoStack::new(10);
        let mut counter = Counter::default();

        counter.value = 1;
        stack.push(UndoEntry::new(
            "set 1",
            |c: &mut Counter| {
                c.value = 0;
                Ok(())
            },
            |c: &mut Counter| {
                c.value = 1;
                Ok(())
            },
        ));

        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        stack.undo(&mut counter).unwrap();
        assert!(stack.can_redo());

        counter.value = 2;
        stack.push(UndoEntry::new(
            "set 2",
            |c: &mut Counter| {
                c.value = 1;
                Ok(())
            },
            |c: &mut Counter| {
                c.value = 2;
                Ok(())
            },
        ));

        assert!(!stack.can_redo());
    }
}
