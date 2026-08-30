use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    sync::atomic::{AtomicBool, Ordering},
};

use wavecrate::sample_sources::{SourceDatabase, SourceFileEvidence, capture_source_file_evidence};
use wavecrate::selection::SelectionRange;
use wavecrate_library::timestamps::system_time_to_unix_nanos;

use crate::native_app::app::{PendingWaveformDestructiveEdit, WaveformDestructiveEditKind};
#[cfg(test)]
use crate::native_app::transaction_history::HistoryFileAction;
use crate::native_app::waveform::{WaveformExtractionRequest, execute_waveform_extraction};
use crate::native_app::waveform_edit_effects::apply_edit_selection_effects;

use self::atomic_write::{AtomicWriteFailure, write_wav_atomically};

mod atomic_write;

#[derive(Debug)]
struct RecoveryDirectory {
    path: PathBuf,
    parent: fs::File,
    name: String,
    identity: String,
    armed: AtomicBool,
}

impl Drop for RecoveryDirectory {
    fn drop(&mut self) {
        if !self.armed.swap(false, Ordering::AcqRel) {
            return;
        }
        #[cfg(not(unix))]
        return;
        #[cfg(unix)]
        {
            let Ok(directory) = open_child_directory(&self.parent, &self.name) else {
                return;
            };
            let Some(identity) =
                wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(
                    &directory,
                )
            else {
                return;
            };
            if identity == self.identity {
                let _ = unlink_child(&directory, "before.wav");
                let _ = unlink_child(&directory, "after.wav");
                let _ = unlink_child(&directory, "extracted.wav");
                let _ = unlink_directory(&self.parent, &self.name);
            }
        }
    }
}

fn create_recovery_directory(
    capability: &crate::native_app::transaction_history::operation_journal::RecoveryRootCapability,
) -> Result<(Arc<RecoveryDirectory>, fs::File), String> {
    let identity =
        wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(
            &capability.file,
        )
        .ok_or_else(|| String::from("recovery root identity unavailable"))?;
    if identity != capability.identity {
        return Err(String::from("recovery root identity changed"));
    }
    let root = capability
        .file
        .try_clone()
        .map_err(|error| error.to_string())?;
    if !root.metadata().map_err(|error| error.to_string())?.is_dir() {
        return Err(String::from("recovery root is not a directory"));
    }
    let recovery = create_directory_at(&root, "session_recovery")?;
    let edits = create_directory_at(&recovery, "waveform_edits")?;
    let mut attempts = 0_u8;
    let (name, edit) = loop {
        if attempts >= 8 {
            return Err(String::from(
                "recovery directory collision budget exhausted",
            ));
        }
        attempts += 1;
        let name = format!("edit-{}", uuid::Uuid::new_v4());
        match create_unique_directory_at(&edits, &name) {
            Ok(edit) => break (name, edit),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    };
    let path = capability
        .path
        .join("session_recovery")
        .join("waveform_edits")
        .join(&name);
    Ok((
        Arc::new(RecoveryDirectory {
            path,
            parent: edits.try_clone().map_err(|error| error.to_string())?,
            name,
            identity:
                wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(
                    &edit,
                )
                .ok_or_else(|| String::from("recovery directory identity unavailable"))?,
            armed: AtomicBool::new(true),
        }),
        edit,
    ))
}

#[cfg(unix)]
fn open_child_directory(parent: &fs::File, name: &str) -> Result<fs::File, String> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = CString::new(name).map_err(|_| String::from("invalid recovery name"))?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().to_string());
    }
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn unlink_child(directory: &fs::File, name: &str) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let name =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL"))?;
    let result = unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn unlink_directory(parent: &fs::File, name: &str) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    let name =
        CString::new(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL"))?;
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn create_directory_at(parent: &fs::File, name: &str) -> Result<fs::File, String> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = CString::new(name).map_err(|_| String::from("invalid recovery name"))?;
        let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error.to_string());
            }
        }
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        return Ok(unsafe { fs::File::from_raw_fd(fd) });
    }
    #[cfg(not(unix))]
    {
        let _ = (parent, name);
        Err(String::from(
            "strict recovery directories unsupported on this platform",
        ))
    }
}

fn create_unique_directory_at(parent: &fs::File, name: &str) -> Result<fs::File, io::Error> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid recovery name"))?;
        let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(unsafe { fs::File::from_raw_fd(fd) });
    }
    #[cfg(not(unix))]
    {
        let _ = (parent, name);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "strict recovery directories unsupported on this platform",
        ))
    }
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct OverwriteBackup {
    pub(in crate::native_app) before: PathBuf,
    pub(in crate::native_app) after: PathBuf,
    dir: Option<Arc<RecoveryDirectory>>,
}

impl PartialEq for OverwriteBackup {
    fn eq(&self, other: &Self) -> bool {
        self.before == other.before
            && self.after == other.after
            && self.dir.as_ref().map(|dir| &dir.path) == other.dir.as_ref().map(|dir| &dir.path)
    }
}

impl OverwriteBackup {
    fn capture_before(
        target: &Path,
        capability: &crate::native_app::transaction_history::operation_journal::RecoveryRootCapability,
    ) -> Result<Self, String> {
        let (dir, edit_directory) = create_recovery_directory(capability)?;
        let before = dir.path.join("before.wav");
        let after = dir.path.join("after.wav");
        let mut source = match crate::native_app::transaction_history::open_no_follow_path(target) {
            Ok(source) => source,
            Err(error) => {
                return Err(error.to_string());
            }
        };
        let source_permissions = source
            .metadata()
            .map_err(|error| error.to_string())?
            .permissions();
        #[cfg(unix)]
        let mut before_file = {
            use std::ffi::CString;
            use std::os::fd::{AsRawFd, FromRawFd};
            let name = CString::new("before.wav").expect("static recovery name");
            let fd = unsafe {
                libc::openat(
                    edit_directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            unsafe { fs::File::from_raw_fd(fd) }
        };
        #[cfg(not(unix))]
        let mut before_file = {
            let _ = edit_directory;
            return Err(String::from(
                "strict recovery files unsupported on this platform",
            ));
        };
        if let Err(error) = io::copy(&mut source, &mut before_file) {
            return Err(format!("Failed to snapshot audio file: {error}"));
        }
        before_file
            .set_permissions(source_permissions)
            .map_err(|error| format!("Failed to preserve snapshot permissions: {error}"))?;
        before_file
            .sync_all()
            .map_err(|error| format!("Failed to sync snapshot: {error}"))?;
        Ok(Self {
            before,
            after,
            dir: Some(dir),
        })
    }

    fn capture_extracted(&self, target: &Path) -> Result<PathBuf, String> {
        let extracted = self
            .dir
            .as_ref()
            .expect("active waveform backup directory")
            .path
            .join("extracted.wav");
        fs::copy(target, &extracted)
            .map_err(|err| format!("Failed to snapshot extracted audio file: {err}"))?;
        Ok(extracted)
    }

    fn retain_recovery_copy(&mut self) {
        if let Some(dir) = self.dir.as_ref() {
            dir.armed.store(false, Ordering::Release);
        }
    }
}

impl Drop for OverwriteBackup {
    fn drop(&mut self) {}
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct AppliedWaveformEdit {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) relative_path: PathBuf,
    pub(in crate::native_app) absolute_path: PathBuf,
    pub(in crate::native_app) before_content_identity: Option<String>,
    pub(in crate::native_app) backup: OverwriteBackup,
    pub(in crate::native_app) extracted: Option<AppliedExtractedFile>,
    pub(in crate::native_app) absolute_evidence: SourceFileEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct AppliedExtractedFile {
    pub(in crate::native_app) path: PathBuf,
    pub(in crate::native_app) relative_path: PathBuf,
    pub(in crate::native_app) backup_path: PathBuf,
    pub(in crate::native_app) evidence: SourceFileEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct WaveformDestructiveEditResult {
    pub(super) result: Result<AppliedWaveformEdit, String>,
    pub(super) extracted_mark: Option<WaveformExtractionMark>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct WaveformExtractionMark {
    pub(super) source_path: PathBuf,
    pub(super) selection: SelectionRange,
}

pub(super) struct WaveformDestructiveEditWorkerRequest {
    edit: PendingWaveformDestructiveEdit,
    extraction: Option<WaveformExtractionRequest>,
    copy_source: Option<PathBuf>,
    // Production does not provide this capability until the participant-lease and recovery
    // lifecycle exists; the current capability-backed worker remains an explicit/test seam.
    recovery_root:
        crate::native_app::transaction_history::operation_journal::RecoveryRootCapability,
}

impl WaveformDestructiveEditWorkerRequest {
    pub(super) fn new(
        edit: PendingWaveformDestructiveEdit,
        extraction: Option<WaveformExtractionRequest>,
        recovery_root: crate::native_app::transaction_history::operation_journal::RecoveryRootCapability,
    ) -> Self {
        Self {
            edit,
            extraction,
            copy_source: None,
            recovery_root,
        }
    }

    pub(super) fn with_copy_source(mut self, source_path: PathBuf) -> Self {
        self.copy_source = Some(source_path);
        self
    }
}

#[derive(Clone)]
struct EditableWav {
    samples: Vec<f32>,
    channels: usize,
    sample_rate: u32,
}

pub(super) fn execute_destructive_edit(
    worker_request: WaveformDestructiveEditWorkerRequest,
) -> WaveformDestructiveEditResult {
    if let Some(source_path) = worker_request.copy_source.as_ref()
        && let Err(error) = prepare_destructive_edit_copy(source_path, &worker_request.edit)
    {
        return WaveformDestructiveEditResult {
            result: Err(error),
            extracted_mark: None,
        };
    }
    let mut extracted_mark = None;
    let extracted_path = match worker_request.extraction {
        Some(extraction) => {
            let completion = execute_waveform_extraction(extraction);
            match completion.result {
                Ok(path) => {
                    extracted_mark = Some(WaveformExtractionMark {
                        source_path: completion.source_path,
                        selection: completion.selection,
                    });
                    Some(path)
                }
                Err(error) => {
                    return WaveformDestructiveEditResult {
                        result: Err(error),
                        extracted_mark: None,
                    };
                }
            }
        }
        None => None,
    };
    let result = match execute_destructive_edit_write(
        &worker_request.edit,
        extracted_path.clone(),
        &worker_request.recovery_root,
    ) {
        Ok(applied) => Ok(applied),
        Err(error) => {
            if let Some(path) = extracted_path {
                cleanup_failed_destructive_extraction(&worker_request.edit.source, &path);
            } else if worker_request.copy_source.is_some() {
                cleanup_failed_destructive_extraction(
                    &worker_request.edit.source,
                    &worker_request.edit.absolute_path,
                );
            }
            Err(error)
        }
    };
    WaveformDestructiveEditResult {
        result,
        extracted_mark,
    }
}

#[cfg(test)]
pub(in crate::native_app) fn execute_destructive_edit_for_tests(
    edit: PendingWaveformDestructiveEdit,
) -> AppliedWaveformEdit {
    let app_root = edit
        .source
        .root
        .parent()
        .expect("test source parent")
        .join(format!(".wavecrate-test-{}", uuid::Uuid::new_v4()));
    let _ = fs::create_dir(&app_root);
    let file = fs::File::open(&app_root).expect("test waveform app root");
    let identity =
        wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(&file)
            .expect("test waveform app root identity");
    execute_destructive_edit(WaveformDestructiveEditWorkerRequest::new(
        edit,
        None,
        crate::native_app::transaction_history::operation_journal::RecoveryRootCapability {
            path: app_root,
            file: Arc::new(file),
            identity,
        },
    ))
    .result
    .expect("destructive edit should succeed")
}

#[cfg(test)]
pub(in crate::native_app) fn destructive_edit_before_backup_path_for_tests(
    applied: &AppliedWaveformEdit,
) -> PathBuf {
    applied.backup.before.clone()
}

#[cfg(test)]
pub(in crate::native_app) fn waveform_restore_action_for_capacity_tests(
    backup_path: PathBuf,
    target_path: PathBuf,
    extracted: bool,
) -> HistoryFileAction {
    HistoryFileAction::WaveformRestore {
        backup_path: backup_path.clone(),
        applied: AppliedWaveformEdit {
            source_id: String::from("test"),
            relative_path: target_path
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("sample.wav")),
            absolute_path: target_path,
            before_content_identity: None,
            backup: OverwriteBackup {
                before: backup_path.clone(),
                after: backup_path.clone(),
                dir: None,
            },
            extracted: extracted.then(|| AppliedExtractedFile {
                path: PathBuf::from("/tmp/extracted.wav"),
                relative_path: PathBuf::from("extracted.wav"),
                backup_path,
                evidence: SourceFileEvidence::Unverifiable,
            }),
            absolute_evidence: SourceFileEvidence::Unverifiable,
        },
    }
}

fn prepare_destructive_edit_copy(
    source_path: &Path,
    request: &PendingWaveformDestructiveEdit,
) -> Result<(), String> {
    if let Some(parent) = request.absolute_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create edit copy folder: {err}"))?;
    }
    fs::copy(source_path, &request.absolute_path).map_err(|err| {
        format!(
            "Failed to copy protected source {} to {}: {err}",
            source_path.display(),
            request.absolute_path.display()
        )
    })?;
    Ok(())
}

pub(super) fn validate_destructive_edit_target(path: &Path) -> Result<(), String> {
    let is_wav = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return Err(String::from("This edit currently supports WAV files"));
    }
    let reader = hound::WavReader::open(path).map_err(|err| format!("Invalid WAV: {err}"))?;
    let channels = reader.spec().channels;
    if channels == 0 || channels > 2 {
        return Err(String::from(
            "This edit currently supports mono or stereo WAV files",
        ));
    }
    Ok(())
}

fn execute_destructive_edit_write(
    request: &PendingWaveformDestructiveEdit,
    extracted_path: Option<PathBuf>,
    recovery_root: &crate::native_app::transaction_history::operation_journal::RecoveryRootCapability,
) -> Result<AppliedWaveformEdit, String> {
    validate_destructive_edit_target(&request.absolute_path)?;
    let before_content_identity = cache_content_identity(&request.absolute_path);
    let mut backup = OverwriteBackup::capture_before(&request.absolute_path, recovery_root)?;
    let extracted = extracted_path
        .map(|path| {
            let relative_path = source_relative_path(&request.source.root, &path)?;
            let backup_path = backup.capture_extracted(&path)?;
            let evidence = capture_source_file_evidence(&path);
            Ok::<_, String>(AppliedExtractedFile {
                path,
                relative_path,
                backup_path,
                evidence,
            })
        })
        .transpose()?;
    let mut wav = load_editable_wav(&request.absolute_path)?;
    apply_destructive_edit_to_wav(&mut wav, request.prompt.edit, request.selection)?;
    if wav.samples.is_empty() {
        return Err(format!(
            "No audio data after {}",
            request.prompt.edit.gerund_label()
        ));
    }
    if let Err(failure) = write_wav_atomically(
        &request.absolute_path,
        &backup.before,
        &backup.after,
        wav.channels,
        wav.sample_rate,
        &wav.samples,
    ) {
        return Err(report_atomic_write_failure(&mut backup, failure));
    }
    Ok(AppliedWaveformEdit {
        source_id: request.source.id.as_str().to_string(),
        relative_path: request.relative_path.clone(),
        absolute_path: request.absolute_path.clone(),
        before_content_identity,
        backup,
        extracted,
        absolute_evidence: capture_source_file_evidence(&request.absolute_path),
    })
}

pub(in crate::native_app) fn restore_edited_waveform(
    backup_path: &Path,
    applied: &AppliedWaveformEdit,
) -> Result<Option<String>, String> {
    let before_content_identity = cache_content_identity(&applied.absolute_path);
    fs::copy(backup_path, &applied.absolute_path)
        .map_err(|err| format!("Failed to restore waveform file: {err}"))?;
    Ok(before_content_identity)
}

pub(in crate::native_app) fn restore_extracted_file_for_transaction(
    backup_path: &Path,
    applied: &AppliedWaveformEdit,
    extracted: &AppliedExtractedFile,
) -> Result<(), String> {
    if backup_path == applied.backup.before.as_path() {
        remove_extracted_file_for_undo(extracted)
    } else {
        restore_extracted_file_for_redo(extracted)
    }
}

fn cache_content_identity(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let modified_ns = system_time_to_unix_nanos(metadata.modified().ok()?);
    Some(format!("cache:{}:{modified_ns}", metadata.len()))
}

fn apply_destructive_edit_to_wav(
    wav: &mut EditableWav,
    edit: WaveformDestructiveEditKind,
    selection: wavecrate::selection::SelectionRange,
) -> Result<(), String> {
    let total_frames = wav.samples.len() / wav.channels.max(1);
    match edit {
        WaveformDestructiveEditKind::CropSelection => {
            crop_wav_to_selection_with_silence(wav, selection)?;
        }
        WaveformDestructiveEditKind::TrimSelection
        | WaveformDestructiveEditKind::ExtractAndTrimSelection => {
            let (start_frame, end_frame) =
                selection_existing_frame_bounds(total_frames, selection)?;
            let start = start_frame * wav.channels;
            let end = end_frame * wav.channels;
            wav.samples.drain(start..end);
        }
        WaveformDestructiveEditKind::ReverseSelection => {
            let (start_frame, end_frame) =
                selection_existing_frame_bounds(total_frames, selection)?;
            let start = start_frame * wav.channels;
            let end = end_frame * wav.channels;
            reverse_interleaved_frames(&mut wav.samples[start..end], wav.channels);
        }
        WaveformDestructiveEditKind::MuteSelection => {
            let (start_frame, end_frame) =
                selection_existing_frame_bounds(total_frames, selection)?;
            let start = start_frame * wav.channels;
            let end = end_frame * wav.channels;
            for sample in &mut wav.samples[start..end] {
                *sample = 0.0;
            }
        }
        WaveformDestructiveEditKind::ApplyEditSelectionEffects => {
            let (start_frame, end_frame) =
                selection_existing_frame_bounds(total_frames, selection)?;
            apply_edit_selection_effects(
                &mut wav.samples,
                wav.channels,
                wav.sample_rate,
                selection,
                start_frame,
                end_frame,
            );
        }
        WaveformDestructiveEditKind::SlideSampleAudio { frame_offset } => {
            slide_interleaved_frames(&mut wav.samples, wav.channels, frame_offset);
        }
    }
    if wav.samples.is_empty() {
        return Err(format!("No audio data after {}", edit.gerund_label()));
    }
    Ok(())
}

fn crop_wav_to_selection_with_silence(
    wav: &mut EditableWav,
    selection: wavecrate::selection::SelectionRange,
) -> Result<(), String> {
    let channels = wav.channels.max(1);
    let total_frames = wav.samples.len() / channels;
    let bounds = selection.signed_frame_bounds(total_frames);
    let output_frames = usize::try_from(bounds.end_frame.saturating_sub(bounds.start_frame).max(1))
        .map_err(|_| String::from("Selected crop range is too large to write"))?;
    let output_samples = output_frames
        .checked_mul(channels)
        .ok_or_else(|| String::from("Selected crop range is too large to write"))?;
    let mut output = vec![0.0; output_samples];
    let source_start_frame = bounds.start_frame.clamp(0, total_frames as i64);
    let source_end_frame = bounds.end_frame.clamp(0, total_frames as i64);
    if source_end_frame > source_start_frame {
        let source_start = source_start_frame as usize * channels;
        let source_end = source_end_frame as usize * channels;
        let target_start = (source_start_frame - bounds.start_frame) as usize * channels;
        let sample_count = source_end.saturating_sub(source_start);
        output[target_start..target_start + sample_count]
            .copy_from_slice(&wav.samples[source_start..source_end]);
    }
    wav.samples = output;
    Ok(())
}

fn reverse_interleaved_frames(samples: &mut [f32], channels: usize) {
    let channels = channels.max(1);
    let frame_count = samples.len() / channels;
    for left_frame in 0..frame_count / 2 {
        let right_frame = frame_count - 1 - left_frame;
        for channel in 0..channels {
            samples.swap(
                left_frame * channels + channel,
                right_frame * channels + channel,
            );
        }
    }
}

fn slide_interleaved_frames(samples: &mut [f32], channels: usize, frame_offset: i64) {
    let channels = channels.max(1);
    let frame_count = samples.len() / channels;
    if frame_count <= 1 {
        return;
    }
    let right_frames = frame_offset.rem_euclid(frame_count as i64) as usize;
    if right_frames == 0 {
        return;
    }
    samples.rotate_right(right_frames * channels);
}

fn load_editable_wav(path: &Path) -> Result<EditableWav, String> {
    let mut reader = hound::WavReader::open(path).map_err(|err| format!("Invalid WAV: {err}"))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("Failed to read WAV samples: {err}"))?,
        hound::SampleFormat::Int => {
            let scale = (1_i64 << spec.bits_per_sample.saturating_sub(1)).max(1) as f32;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| value as f32 / scale)
                        .map_err(|err| format!("Failed to read WAV samples: {err}"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    if samples.is_empty() {
        return Err(String::from("No audio data available"));
    }
    Ok(EditableWav {
        samples,
        channels,
        sample_rate: spec.sample_rate.max(1),
    })
}

fn report_atomic_write_failure(
    backup: &mut OverwriteBackup,
    failure: AtomicWriteFailure,
) -> String {
    if failure.recovery_copy_required() {
        backup.retain_recovery_copy();
    }
    failure.to_string()
}

fn selection_existing_frame_bounds(
    total_frames: usize,
    bounds: wavecrate::selection::SelectionRange,
) -> Result<(usize, usize), String> {
    if total_frames == 0 {
        return Err(String::from("No audio data available"));
    }
    let authored = bounds.signed_frame_bounds(total_frames);
    let start_frame = authored.start_frame.clamp(0, total_frames as i64) as usize;
    let end_frame = authored.end_frame.clamp(0, total_frames as i64) as usize;
    if end_frame <= start_frame {
        return Err(String::from("Selection is outside the audio data"));
    }
    Ok((start_frame, end_frame))
}

fn source_relative_path(source_root: &Path, absolute_path: &Path) -> Result<PathBuf, String> {
    absolute_path
        .strip_prefix(source_root)
        .map(Path::to_path_buf)
        .map_err(|_| String::from("Edited sample is not inside the configured source"))
}

fn remove_extracted_file_for_undo(extracted: &AppliedExtractedFile) -> Result<(), String> {
    match fs::remove_file(&extracted.path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to remove extracted audio file: {error}")),
    }
    Ok(())
}

fn cleanup_failed_destructive_extraction(
    source: &wavecrate::sample_sources::SampleSource,
    extracted_path: &Path,
) {
    let _ = fs::remove_file(extracted_path);
    let Ok(relative_path) = source_relative_path(&source.root, extracted_path) else {
        return;
    };
    let Ok(database_root) = source.database_root() else {
        return;
    };
    let _ = mark_source_entry_missing_at(&source.root, &database_root, &relative_path, true);
}

fn restore_extracted_file_for_redo(extracted: &AppliedExtractedFile) -> Result<(), String> {
    fs::copy(&extracted.backup_path, &extracted.path)
        .map(|_| ())
        .map_err(|err| format!("Failed to restore extracted audio file: {err}"))
}

fn mark_source_entry_missing_at(
    source_root: &Path,
    database_root: &Path,
    relative_path: &Path,
    missing: bool,
) -> Result<(), String> {
    SourceDatabase::open_for_source_write_with_database_root(source_root, database_root)
        .map_err(|err| format!("Database unavailable: {err}"))?
        .set_missing(relative_path, missing)
        .map_err(|err| format!("Failed to sync database entry: {err}"))
}
