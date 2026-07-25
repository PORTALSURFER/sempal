use std::path::Path;

use super::super::{SourceDbError, SourceWriteBatch};
use super::mutation::{
    delete_path_statement, remap_analysis_sample_identity_statement, remap_wav_file_path_statement,
};

impl SourceWriteBatch<'_> {
    /// Remove a wav row within the batch.
    pub fn remove_file(&mut self, relative_path: &Path) -> Result<(), SourceDbError> {
        self.paths_revision_dirty = true;
        self.identities_revision_dirty = true;
        self.manifest_touched_paths
            .insert(relative_path.to_path_buf());
        delete_path_statement(&self.tx, relative_path)
    }

    /// Remap a wav row and its path-keyed user metadata after a filesystem rename.
    pub fn remap_wav_file_path(
        &mut self,
        old_relative_path: &Path,
        new_relative_path: &Path,
    ) -> Result<(), SourceDbError> {
        self.paths_revision_dirty = true;
        self.identities_revision_dirty = true;
        self.manifest_touched_paths
            .insert(old_relative_path.to_path_buf());
        self.manifest_touched_paths
            .insert(new_relative_path.to_path_buf());
        self.clear_pending_rename(old_relative_path)?;
        self.clear_pending_rename(new_relative_path)?;
        remap_wav_file_path_statement(&self.tx, old_relative_path, new_relative_path)
    }

    /// Remap path-derived analysis rows after a rename-only sample identity change.
    pub fn remap_analysis_sample_identity(
        &mut self,
        old_relative_path: &Path,
        new_relative_path: &Path,
    ) -> Result<(), SourceDbError> {
        remap_analysis_sample_identity_statement(&self.tx, old_relative_path, new_relative_path)
    }
}
