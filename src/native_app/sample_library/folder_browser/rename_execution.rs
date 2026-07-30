use std::fs;

use wavecrate::sample_sources::{SourceDatabase, capture_source_file_evidence};

use super::{
    FileMetadataRemap, FolderEntry, RenameCommitCompletion, RenameCommitRequest,
    RenameCommitSuccess,
    path_helpers::{folder_label, path_id},
};

pub(in crate::native_app) fn execute_rename_commit_request(
    request: RenameCommitRequest,
) -> RenameCommitCompletion {
    let mut post_filesystem_evidence = None;
    let result = match &request {
        RenameCommitRequest::FolderRename {
            old_path,
            new_path,
            new_name,
        } => {
            if new_path.exists() {
                Err(format!("Folder rename failed: {new_name} already exists"))
            } else {
                fs::rename(old_path, new_path)
                    .map(|()| {
                        post_filesystem_evidence = Some(capture_source_file_evidence(new_path));
                        RenameCommitSuccess::FolderRenamed
                    })
                    .map_err(|error| format!("Folder rename failed: {error}"))
            }
        }
        RenameCommitRequest::FolderCreate {
            new_path, new_name, ..
        } => {
            if new_path.exists() {
                Err(format!("New folder failed: {new_name} already exists"))
            } else {
                fs::create_dir(new_path)
                    .map(|()| RenameCommitSuccess::FolderCreated {
                        folder: FolderEntry {
                            id: path_id(new_path),
                            name: folder_label(new_path),
                            children: Vec::new(),
                            files: Vec::new(),
                        },
                    })
                    .map_err(|error| format!("New folder failed: {error}"))
            }
        }
        RenameCommitRequest::FileRename {
            old_path,
            new_path,
            new_name,
            metadata_remap,
        } => {
            if new_path.exists() {
                Err(format!("File rename failed: {new_name} already exists"))
            } else {
                fs::rename(old_path, new_path)
                    .map(|()| {
                        post_filesystem_evidence = Some(capture_source_file_evidence(new_path));
                        RenameCommitSuccess::FileRenamed {
                            metadata_remap_result: metadata_remap
                                .as_ref()
                                .map(persist_renamed_file_metadata)
                                .unwrap_or(Ok(())),
                        }
                    })
                    .map_err(|error| format!("File rename failed: {error}"))
            }
        }
    };
    RenameCommitCompletion {
        request,
        result,
        post_filesystem_evidence,
    }
}

fn persist_renamed_file_metadata(remap: &FileMetadataRemap) -> Result<(), String> {
    let db = SourceDatabase::open_for_user_metadata_write(&remap.source_root)
        .map_err(|err| err.to_string())?;
    let mut batch = db.write_batch().map_err(|err| err.to_string())?;
    batch
        .remap_wav_file_path(&remap.old_relative, &remap.new_relative)
        .map_err(|err| err.to_string())?;
    batch
        .remap_analysis_sample_identity(&remap.old_relative, &remap.new_relative)
        .map_err(|err| err.to_string())?;
    batch.commit().map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecrate::sample_sources::SourceFileEvidence;

    #[test]
    fn rename_worker_captures_destination_evidence_after_filesystem_success() {
        let root = tempfile::tempdir().expect("rename root");
        let old_path = root.path().join("old");
        let new_path = root.path().join("new");
        std::fs::create_dir(&old_path).expect("create old folder");

        let completion = execute_rename_commit_request(RenameCommitRequest::FolderRename {
            old_path,
            new_path,
            new_name: String::from("new"),
        });

        assert!(matches!(
            completion.result,
            Ok(RenameCommitSuccess::FolderRenamed)
        ));
        assert!(matches!(
            completion.post_filesystem_evidence,
            Some(SourceFileEvidence::Metadata { is_dir: true, .. })
        ));
    }
}
