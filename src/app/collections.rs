use super::*;
use crate::ui::{CollectionRow, CollectionSampleRow};
use slint::{SharedString, VecModel};
use std::path::PathBuf;
use std::rc::Rc;

/// Shared models for the collection list and member list.
pub(super) struct CollectionModels {
    pub list: Rc<VecModel<CollectionRow>>,
    pub members: Rc<VecModel<CollectionSampleRow>>,
}

impl CollectionModels {
    pub fn new() -> Self {
        Self {
            list: Rc::new(VecModel::default()),
            members: Rc::new(VecModel::default()),
        }
    }
}

impl DropHandler {
    /// Push current collections into the UI and ensure selection is reflected.
    pub(super) fn refresh_collections(&self, app: &Sempal) {
        let selected = self.selected_collection.borrow().clone();
        let rows = self
            .collections
            .borrow()
            .iter()
            .map(|collection| CollectionRow {
                id: collection.id.as_str().into(),
                name: collection.name.clone().into(),
                selected: selected.as_ref().is_some_and(|id| id == &collection.id),
                count: collection.members.len() as i32,
            })
            .collect::<Vec<_>>();
        let model = Rc::new(VecModel::from(rows));
        self.collection_models.borrow_mut().list = model.clone();
        app.set_collections(model.into());
        let selected_index = selected
            .and_then(|id| self.collections.borrow().iter().position(|c| c.id == id))
            .map(|i| i as i32)
            .unwrap_or(-1);
        app.set_selected_collection(selected_index);
        self.refresh_collection_members(app);
    }

    /// Update the member list for the selected collection.
    pub(super) fn refresh_collection_members(&self, app: &Sempal) {
        let selected = self.selected_collection.borrow().clone();
        let rows = selected
            .and_then(|id| {
                self.collections
                    .borrow()
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
            })
            .map(|collection| {
                collection
                    .members
                    .iter()
                    .map(|member| {
                        let source_label = self
                            .sources
                            .borrow()
                            .iter()
                            .find(|s| s.id == member.source_id)
                            .map(|source| {
                                source
                                    .root
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|n| n.to_string())
                                    .unwrap_or_else(|| source.root.to_string_lossy().to_string())
                            })
                            .unwrap_or_else(|| "Unknown source".to_string());
                        let path = member.relative_path.to_string_lossy().to_string();
                        CollectionSampleRow {
                            source: source_label.into(),
                            path: path.into(),
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let model = Rc::new(VecModel::from(rows));
        self.collection_models.borrow_mut().members = model.clone();
        app.set_collection_samples(model.into());
    }

    /// Ensure a selection exists after loading collections.
    pub(super) fn ensure_collection_selection(&self) {
        if self.selected_collection.borrow().is_some() {
            return;
        }
        if let Some(first) = self.collections.borrow().first().cloned() {
            self.selected_collection
                .borrow_mut()
                .replace(first.id.clone());
        }
    }

    /// Add a new collection with an auto-generated name.
    pub fn handle_add_collection(&self) {
        if !self.feature_flags.borrow().collections_enabled {
            return;
        }
        let Some(app) = self.app() else {
            return;
        };
        let name = self.next_collection_name();
        let mut collections = self.collections.borrow_mut();
        let collection = Collection::new(name);
        let id = collection.id.clone();
        collections.push(collection);
        drop(collections);
        self.selected_collection.borrow_mut().replace(id.clone());
        if let Err(error) = self.save_full_config() {
            self.set_status(
                &app,
                format!("Failed to save collection: {error}"),
                StatusState::Error,
            );
            return;
        }
        self.refresh_collections(&app);
        self.set_status(&app, "Collection created", StatusState::Info);
    }

    /// Switch which collection is selected.
    pub fn handle_collection_selected(&self, index: i32) {
        if index < 0 {
            self.selected_collection.borrow_mut().take();
        } else if let Some(collection) = self.collections.borrow().get(index as usize).cloned() {
            self.selected_collection
                .borrow_mut()
                .replace(collection.id.clone());
        }
        if let Some(app) = self.app() {
            self.refresh_collections(&app);
        }
    }

    /// Add a dropped sample (relative path) from the current source into the collection.
    pub fn handle_sample_dropped_on_collection(
        &self,
        collection_id: SharedString,
        relative_path: SharedString,
    ) {
        if !self.feature_flags.borrow().collections_enabled {
            return;
        }
        let Some(app) = self.app() else {
            return;
        };
        let Some(source) = self.current_source() else {
            self.set_status(
                &app,
                "Select a source to add samples to collections",
                StatusState::Warning,
            );
            app.set_dragging_sample_path("".into());
            return;
        };
        let mut collections = self.collections.borrow_mut();
        let Some(collection) = collections
            .iter_mut()
            .find(|c| c.id.as_str() == collection_id.as_str())
        else {
            app.set_dragging_sample_path("".into());
            return;
        };
        let path = PathBuf::from(relative_path.as_str());
        if !self.wav_lookup.borrow().contains_key(&path) {
            self.set_status(&app, "Sample is not available to add", StatusState::Warning);
            app.set_dragging_sample_path("".into());
            return;
        }
        let added = collection.add_member(source.id.clone(), path.clone());
        drop(collections);
        if added {
            self.persist_collection_add(&app, path);
        } else {
            self.set_status(&app, "Already in collection", StatusState::Info);
        }
        app.set_drag_preview_visible(false);
        app.set_drag_preview_label("".into());
        self.refresh_collections(&app);
        app.set_dragging_sample_path("".into());
    }

    /// Drop any members tied to a removed source.
    pub(super) fn prune_collections_for_source(&self, source_id: &SourceId) {
        self.collections
            .borrow_mut()
            .iter_mut()
            .for_each(|collection| collection.prune_source(source_id));
    }

    fn persist_collection_add(&self, app: &Sempal, path: PathBuf) {
        if let Err(error) = self.save_full_config() {
            self.set_status(
                app,
                format!("Failed to save collection: {error}"),
                StatusState::Error,
            );
            return;
        }
        self.set_status(
            app,
            format!("Added {} to collection", path.display()),
            StatusState::Info,
        );
    }

    fn next_collection_name(&self) -> String {
        let base = "Collection";
        let mut index = self.collections.borrow().len() + 1;
        loop {
            let candidate = format!("{base} {index}");
            if !self
                .collections
                .borrow()
                .iter()
                .any(|c| c.name == candidate)
            {
                return candidate;
            }
            index += 1;
        }
    }
}
