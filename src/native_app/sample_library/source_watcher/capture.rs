#![cfg_attr(not(test), allow(dead_code))]

use notify::{
    Event, EventKind,
    event::{EventAttributes, Flag},
};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};
use wavecrate::sample_sources::SourceId;
use wavecrate_library::sample_sources::reconciliation::{
    BackendStreamIdentity, CaptureBoundary, ObservationUncertainty, RawEventKind, RawObservation,
    RawObservationLimits, RawObservationMetadata, RawObservationProvenance, RawObservedPath,
    RawPathHint, RawPathRole, RootIdentity, RootRelativePath, RootRelativePathError,
    SyntheticObservationBatch, WatcherGeneration,
};

pub(super) const MAX_CAPTURE_PATHS: usize = 4_096;
pub(super) const MAX_CAPTURE_PATH_BYTES: usize = 256 * 1_024;
pub(super) const MAX_CAPTURE_METADATA_BYTES: usize = 256 * 1_024;
const MAX_RAW_OBSERVATIONS_PER_CAPTURE: usize = 1;
const RAW_FLAG_RESCAN: u64 = 1 << 0;
const RAW_EVENT_KIND_METADATA_BYTES: usize = 1;
const RAW_UNCERTAINTY_METADATA_BYTES: usize = std::mem::size_of::<u16>();
const RAW_PATH_ROLE_AND_HINT_METADATA_BYTES: usize = 2;

#[derive(Debug)]
pub(super) enum SourceWatcherCapture {
    Notify { stream_id: u64, event: Event },
    Error { stream_id: u64 },
    Overflow { stream_id: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CaptureAdmissionError {
    PathOutsideSourceRoot {
        path: PathBuf,
    },
    InvalidRelativePath {
        path: PathBuf,
        error: RootRelativePathError,
    },
    MetadataValueOverflow {
        field: &'static str,
        value: usize,
    },
    ReplayPathCountExceeded {
        value: usize,
    },
    ReplayPathBytesExceeded {
        value: usize,
    },
}

pub(super) fn capture_event(event: notify::Result<Event>) -> SourceWatcherCapture {
    let Ok(captured_event) = event else {
        return SourceWatcherCapture::Error { stream_id: 0 };
    };

    if captured_event.paths.len() > MAX_CAPTURE_PATHS {
        return SourceWatcherCapture::Overflow { stream_id: 0 };
    }

    let path_bytes = captured_event.paths.iter().try_fold(0usize, |total, path| {
        let total = total.checked_add(path.as_os_str().as_encoded_bytes().len())?;
        (total <= MAX_CAPTURE_PATH_BYTES).then_some(total)
    });
    let metadata_bytes = raw_metadata_bytes(&captured_event);
    if path_bytes.is_none()
        || metadata_bytes.map_or(true, |bytes| bytes > MAX_CAPTURE_METADATA_BYTES)
    {
        SourceWatcherCapture::Overflow { stream_id: 0 }
    } else {
        SourceWatcherCapture::Notify {
            stream_id: 0,
            event: captured_event,
        }
    }
}

/// Conservatively account for every raw metadata byte this adapter can retain.
///
/// The raw model always charges the event kind and may charge uncertainty, so those bytes are
/// reserved for every event. Each path reserves both its role and optional hint byte. Optional
/// Notify attributes are accounted exactly when the callback can retain them.
fn raw_metadata_bytes(event: &Event) -> Option<usize> {
    let path_metadata_bytes = event
        .paths
        .len()
        .checked_mul(RAW_PATH_ROLE_AND_HINT_METADATA_BYTES)?;
    let mut total = RAW_EVENT_KIND_METADATA_BYTES
        .checked_add(RAW_UNCERTAINTY_METADATA_BYTES)?
        .checked_add(path_metadata_bytes)?;

    if event.attrs.flag() == Some(Flag::Rescan) {
        total = total.checked_add(std::mem::size_of::<u64>())?;
    }
    if event.attrs.tracker().is_some() {
        total = total.checked_add(std::mem::size_of::<u64>())?;
    }
    if event.attrs.process_id().is_some() {
        total = total.checked_add(std::mem::size_of::<u32>())?;
    }

    event
        .attrs
        .info()
        .into_iter()
        .chain(event.attrs.source())
        .try_fold(total, |total, metadata| {
            total.checked_add(metadata.as_bytes().len())
        })
}

/// Convert one bounded native capture into the pure reconciliation admission shape.
///
/// The capture already carries its backend stream identity. A notify event is converted using
/// lexical path operations only; the source root is never read or canonicalized here.
pub(super) fn capture_to_observation_batch(
    capture: SourceWatcherCapture,
    source_root: &Path,
    source_id: SourceId,
    root_identity: RootIdentity,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
) -> Result<SyntheticObservationBatch, CaptureAdmissionError> {
    match capture {
        SourceWatcherCapture::Notify { stream_id, event } => event_observation_batch(
            event,
            source_root,
            source_id,
            root_identity,
            stream_id,
            watcher_generation,
            capture_boundary,
        ),
        SourceWatcherCapture::Error { stream_id } => Ok(marker_batch(
            RawEventKind::Error,
            ObservationUncertainty::BACKEND_ERROR,
            source_id,
            root_identity,
            stream_id,
            watcher_generation,
            capture_boundary,
        )),
        SourceWatcherCapture::Overflow { stream_id } => Ok(marker_batch(
            RawEventKind::Overflow,
            ObservationUncertainty::OVERFLOW,
            source_id,
            root_identity,
            stream_id,
            watcher_generation,
            capture_boundary,
        )),
    }
}

/// Convert a notify event while retaining bounded overflow as a typed raw marker.
pub(super) fn event_to_observation_batch(
    event: Event,
    source_root: &Path,
    source_id: SourceId,
    root_identity: RootIdentity,
    stream_id: u64,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
) -> Result<SyntheticObservationBatch, CaptureAdmissionError> {
    let bounded = capture_event(Ok(event));
    let capture = match bounded {
        SourceWatcherCapture::Notify { event, .. } => {
            SourceWatcherCapture::Notify { stream_id, event }
        }
        SourceWatcherCapture::Error { .. } => SourceWatcherCapture::Error { stream_id },
        SourceWatcherCapture::Overflow { .. } => SourceWatcherCapture::Overflow { stream_id },
    };
    capture_to_observation_batch(
        capture,
        source_root,
        source_id,
        root_identity,
        watcher_generation,
        capture_boundary,
    )
}

fn event_observation(
    event: Event,
    source_root: &Path,
) -> Result<RawObservation, CaptureAdmissionError> {
    let rescan = event.attrs.flag() == Some(Flag::Rescan);
    let metadata = observation_metadata(&event.attrs)?;
    event_observation_with_metadata(event, source_root, metadata, rescan)
}

fn event_observation_with_metadata(
    event: Event,
    source_root: &Path,
    metadata: RawObservationMetadata,
    rescan: bool,
) -> Result<RawObservation, CaptureAdmissionError> {
    let Event { kind, paths, .. } = event;
    let path_count = paths.len();
    let mapping = map_event_kind(kind, path_count);
    let mut relative_paths = Vec::with_capacity(path_count);
    let mut root_observed = false;

    for path in paths {
        match relative_path(source_root, path)? {
            Some(path) => relative_paths.push(path),
            None => root_observed = true,
        }
    }

    if root_observed {
        let observation = RawObservation::new(
            RawEventKind::RootChanged,
            relative_paths
                .into_iter()
                .map(|path| RawObservedPath::new(path, RawPathRole::Subject))
                .collect(),
        )
        .with_metadata(metadata);
        let mut uncertainty = mapping.uncertainty | ObservationUncertainty::PATH_COVERAGE;
        if rescan {
            uncertainty |= ObservationUncertainty::PATH_COVERAGE;
        }
        return Ok(observation.with_uncertainty(uncertainty));
    }

    let mut uncertainty = mapping.uncertainty;
    if relative_paths.is_empty() {
        uncertainty |= ObservationUncertainty::PATH_COVERAGE;
    }
    if rescan {
        uncertainty |= ObservationUncertainty::PATH_COVERAGE;
    }

    let observed_paths = relative_paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let observed = RawObservedPath::new(path, mapping.role(index));
            match mapping.hint {
                Some(hint) => observed.with_hint(hint),
                None => observed,
            }
        })
        .collect();
    Ok(RawObservation::new(mapping.kind, observed_paths)
        .with_metadata(metadata)
        .with_uncertainty(uncertainty))
}

fn observation_metadata(
    attrs: &EventAttributes,
) -> Result<RawObservationMetadata, CaptureAdmissionError> {
    observation_metadata_from_values(
        attrs.flag(),
        attrs.tracker(),
        attrs.info(),
        attrs.source(),
        attrs.process_id(),
    )
}

fn observation_metadata_from_values(
    flag: Option<Flag>,
    tracker: Option<usize>,
    info: Option<&str>,
    source: Option<&str>,
    process_id: Option<u32>,
) -> Result<RawObservationMetadata, CaptureAdmissionError> {
    let mut metadata = RawObservationMetadata::new();
    if flag == Some(Flag::Rescan) {
        metadata = metadata.with_flags(RAW_FLAG_RESCAN);
    }
    if let Some(tracker) = tracker {
        let tracker =
            u64::try_from(tracker).map_err(|_| CaptureAdmissionError::MetadataValueOverflow {
                field: "tracker",
                value: tracker,
            })?;
        metadata = metadata.with_rename_cookie(tracker);
    }
    if let Some(info) = info {
        metadata = metadata.with_detail(OsString::from(info));
    }
    if let Some(source) = source {
        metadata = metadata.with_source(OsString::from(source));
    }
    if let Some(process_id) = process_id {
        metadata = metadata.with_process_id(process_id);
    }
    Ok(metadata)
}

fn event_observation_batch(
    event: Event,
    source_root: &Path,
    source_id: SourceId,
    root_identity: RootIdentity,
    stream_id: u64,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
) -> Result<SyntheticObservationBatch, CaptureAdmissionError> {
    let observation = event_observation(event, source_root)?;
    Ok(batch(
        observation,
        source_id,
        root_identity,
        stream_id,
        watcher_generation,
        capture_boundary,
    ))
}

fn marker_batch(
    kind: RawEventKind,
    uncertainty: ObservationUncertainty,
    source_id: SourceId,
    root_identity: RootIdentity,
    stream_id: u64,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
) -> SyntheticObservationBatch {
    batch(
        RawObservation::new(kind, Vec::new()).with_uncertainty(uncertainty),
        source_id,
        root_identity,
        stream_id,
        watcher_generation,
        capture_boundary,
    )
}

fn batch(
    observation: RawObservation,
    source_id: SourceId,
    root_identity: RootIdentity,
    stream_id: u64,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
) -> SyntheticObservationBatch {
    let provenance = RawObservationProvenance::new(
        source_id,
        Some(root_identity),
        Some(BackendStreamIdentity::from_bytes(
            stream_id.to_be_bytes().to_vec(),
        )),
        watcher_generation,
        capture_boundary,
    );
    SyntheticObservationBatch::new(
        provenance,
        vec![observation],
        native_raw_observation_limits(),
    )
}

fn native_raw_observation_limits() -> RawObservationLimits {
    RawObservationLimits::new(
        MAX_RAW_OBSERVATIONS_PER_CAPTURE,
        MAX_CAPTURE_PATH_BYTES,
        MAX_CAPTURE_METADATA_BYTES,
    )
    .expect("native raw observation limits")
}

/// Convert bounded FSEvents path history into one conservative raw observation.
///
/// FSEvents history recovery retains paths and coverage, but this adapter deliberately does not
/// invent create/delete/rename semantics from path-only evidence. The source synchronizer observes
/// current truth for every retained path, while the owner admission layer supplies continuity.
pub(super) fn fsevents_replay_observations(
    paths: Vec<PathBuf>,
) -> Result<(Vec<RawObservation>, RawObservationLimits), CaptureAdmissionError> {
    if paths.len() > MAX_CAPTURE_PATHS {
        return Err(CaptureAdmissionError::ReplayPathCountExceeded { value: paths.len() });
    }
    let mut path_bytes = 0usize;
    let mut observed_paths = Vec::with_capacity(paths.len());
    for path in paths {
        if path.as_os_str().is_empty() {
            continue;
        }
        path_bytes = path_bytes
            .checked_add(path.as_os_str().as_encoded_bytes().len())
            .ok_or(CaptureAdmissionError::ReplayPathBytesExceeded { value: usize::MAX })?;
        if path_bytes > MAX_CAPTURE_PATH_BYTES {
            return Err(CaptureAdmissionError::ReplayPathBytesExceeded { value: path_bytes });
        }
        observed_paths.push(RawObservedPath::new(path, RawPathRole::Subject));
    }
    if observed_paths.is_empty() {
        return Err(CaptureAdmissionError::ReplayPathCountExceeded { value: 0 });
    }
    Ok((
        vec![RawObservation::new(RawEventKind::Modify, observed_paths)],
        native_raw_observation_limits(),
    ))
}

#[derive(Clone, Copy)]
struct EventMapping {
    kind: RawEventKind,
    path_shape: PathShape,
    uncertainty: ObservationUncertainty,
    hint: Option<RawPathHint>,
}

#[derive(Clone, Copy)]
enum PathShape {
    Subject,
    RenameBoth,
    RenameSource,
    RenameDestination,
}

impl EventMapping {
    fn role(self, index: usize) -> RawPathRole {
        match self.path_shape {
            PathShape::Subject => RawPathRole::Subject,
            PathShape::RenameBoth => match index {
                0 => RawPathRole::RenameSource,
                _ => RawPathRole::RenameDestination,
            },
            PathShape::RenameSource => RawPathRole::RenameSource,
            PathShape::RenameDestination => RawPathRole::RenameDestination,
        }
    }
}

fn map_event_kind(kind: EventKind, path_count: usize) -> EventMapping {
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode};

    match kind {
        EventKind::Create(create_kind) => match create_kind {
            CreateKind::File => EventMapping {
                kind: RawEventKind::Create,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: None,
            },
            CreateKind::Folder => EventMapping {
                kind: RawEventKind::Create,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: Some(RawPathHint::Directory),
            },
            CreateKind::Any => EventMapping {
                kind: RawEventKind::Create,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: Some(RawPathHint::Unknown),
            },
            CreateKind::Other => unsupported_mapping(),
        },
        EventKind::Modify(modify_kind) => match modify_kind {
            ModifyKind::Data(DataChange::Any | DataChange::Size | DataChange::Content)
            | ModifyKind::Metadata(MetadataKind::Any)
            | ModifyKind::Metadata(MetadataKind::AccessTime)
            | ModifyKind::Metadata(MetadataKind::WriteTime)
            | ModifyKind::Metadata(MetadataKind::Permissions)
            | ModifyKind::Metadata(MetadataKind::Ownership)
            | ModifyKind::Metadata(MetadataKind::Extended)
            | ModifyKind::Metadata(MetadataKind::Other)
            | ModifyKind::Any => EventMapping {
                kind: RawEventKind::Modify,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: None,
            },
            ModifyKind::Name(rename_mode) => match rename_mode {
                RenameMode::Both if path_count == 2 => EventMapping {
                    kind: RawEventKind::Rename,
                    path_shape: PathShape::RenameBoth,
                    uncertainty: ObservationUncertainty::empty(),
                    hint: None,
                },
                RenameMode::From if path_count == 1 => EventMapping {
                    kind: RawEventKind::Rename,
                    path_shape: PathShape::RenameSource,
                    uncertainty: ObservationUncertainty::PATH_COVERAGE,
                    hint: None,
                },
                RenameMode::To if path_count == 1 => EventMapping {
                    kind: RawEventKind::Rename,
                    path_shape: PathShape::RenameDestination,
                    uncertainty: ObservationUncertainty::PATH_COVERAGE,
                    hint: None,
                },
                RenameMode::Any
                | RenameMode::From
                | RenameMode::To
                | RenameMode::Both
                | RenameMode::Other => EventMapping {
                    kind: RawEventKind::Rename,
                    path_shape: PathShape::Subject,
                    uncertainty: ObservationUncertainty::PATH_COVERAGE,
                    hint: None,
                },
            },
            ModifyKind::Data(DataChange::Other) | ModifyKind::Other => unsupported_mapping(),
        },
        EventKind::Remove(remove_kind) => match remove_kind {
            RemoveKind::File => EventMapping {
                kind: RawEventKind::Delete,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: None,
            },
            RemoveKind::Folder => EventMapping {
                kind: RawEventKind::Delete,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: Some(RawPathHint::Directory),
            },
            RemoveKind::Any => EventMapping {
                kind: RawEventKind::Delete,
                path_shape: PathShape::Subject,
                uncertainty: ObservationUncertainty::empty(),
                hint: Some(RawPathHint::Unknown),
            },
            RemoveKind::Other => unsupported_mapping(),
        },
        EventKind::Any | EventKind::Access(_) | EventKind::Other => unsupported_mapping(),
    }
}

fn unsupported_mapping() -> EventMapping {
    EventMapping {
        kind: RawEventKind::Unsupported,
        path_shape: PathShape::Subject,
        uncertainty: ObservationUncertainty::PATH_COVERAGE,
        hint: None,
    }
}

fn relative_path(
    source_root: &Path,
    path: PathBuf,
) -> Result<Option<PathBuf>, CaptureAdmissionError> {
    let relative = path
        .strip_prefix(source_root)
        .map_err(|_| CaptureAdmissionError::PathOutsideSourceRoot { path: path.clone() })?
        .to_path_buf();
    if relative
        .components()
        .all(|component| matches!(component, Component::CurDir))
    {
        return Ok(None);
    }
    RootRelativePath::try_from_path(relative.clone())
        .map(RootRelativePath::into_path)
        .map(Some)
        .map_err(|error| CaptureAdmissionError::InvalidRelativePath { path, error })
}

#[cfg(test)]
mod tests {
    use super::{
        CaptureAdmissionError, MAX_CAPTURE_METADATA_BYTES, MAX_CAPTURE_PATH_BYTES,
        MAX_CAPTURE_PATHS, RAW_PATH_ROLE_AND_HINT_METADATA_BYTES, SourceWatcherCapture,
        capture_event, capture_to_observation_batch, event_observation_with_metadata,
        event_to_observation_batch, fsevents_replay_observations, observation_metadata_from_values,
    };
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, EventAttributes, Flag, ModifyKind,
        RemoveKind, RenameMode,
    };
    use notify::{Event, EventKind};
    use std::path::{Path, PathBuf};
    use wavecrate::sample_sources::SourceId;
    use wavecrate_library::sample_sources::reconciliation::{
        CaptureBoundary, ObservationUncertainty, RawEventKind, RawObservationEnvelope, RawPathHint,
        RawPathRole, RootIdentity, RootRelativePathError, WatcherGeneration,
    };

    fn boundary() -> CaptureBoundary {
        CaptureBoundary::try_new(123, None, None).expect("capture boundary")
    }

    fn event_at_raw_metadata_boundary(metadata_bytes: usize) -> Event {
        let fixed_metadata_bytes = 1usize
            .checked_add(std::mem::size_of::<u16>())
            .and_then(|bytes| bytes.checked_add(RAW_PATH_ROLE_AND_HINT_METADATA_BYTES))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<u64>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<u64>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<u32>()))
            .expect("fixed metadata bytes");
        let info_bytes = metadata_bytes
            .checked_sub(fixed_metadata_bytes)
            .expect("metadata boundary has room for fixed fields");

        let mut attrs = EventAttributes::new();
        attrs.set_flag(Flag::Rescan);
        attrs.set_tracker(73);
        attrs.set_info(&"i".repeat(info_bytes));
        attrs.set_process_id(42);
        Event {
            kind: EventKind::Create(CreateKind::Folder),
            paths: vec![PathBuf::from("source-root/folder")],
            attrs,
        }
    }

    fn event(kind: EventKind, root: &Path, names: &[&str]) -> Event {
        Event {
            kind,
            paths: names.iter().map(|name| root.join(name)).collect(),
            attrs: EventAttributes::default(),
        }
    }

    fn observation(
        event: Event,
        root: &Path,
    ) -> wavecrate_library::sample_sources::reconciliation::RawObservation {
        event_to_observation_batch(
            event,
            root,
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            7,
            WatcherGeneration::new(3),
            boundary(),
        )
        .expect("observation batch")
        .observations()[0]
            .clone()
    }

    #[test]
    fn accepted_event_preserves_kind_paths_and_attributes() {
        let paths = vec![PathBuf::from("first.wav"), PathBuf::from("second.wav")];
        let mut attrs = EventAttributes::new();
        attrs.set_tracker(7);
        attrs.set_info("capture-test");
        attrs.set_process_id(42);
        let event = Event {
            kind: EventKind::Any,
            paths: paths.clone(),
            attrs,
        };

        let SourceWatcherCapture::Notify { event, .. } = capture_event(Ok(event)) else {
            panic!("event should be accepted");
        };
        assert_eq!(event.kind, EventKind::Any);
        assert_eq!(event.paths, paths);
        assert_eq!(event.attrs.tracker(), Some(7));
        assert_eq!(event.attrs.info(), Some("capture-test"));
        assert_eq!(event.attrs.process_id(), Some(42));
    }

    #[cfg(unix)]
    #[test]
    fn accepted_event_preserves_native_non_utf8_path_order() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let first = PathBuf::from(OsString::from_vec(vec![
            b'f', b'i', b'r', b's', b't', 0xff, b'.', b'w', b'a', b'v',
        ]));
        let second = PathBuf::from("second.wav");
        let paths = vec![first, second];
        let event = Event {
            kind: EventKind::Any,
            paths: paths.clone(),
            attrs: EventAttributes::default(),
        };

        let SourceWatcherCapture::Notify { event, .. } = capture_event(Ok(event)) else {
            panic!("event should be accepted");
        };
        assert_eq!(event.paths, paths);
        assert_eq!(
            event.paths[0].as_os_str().as_encoded_bytes(),
            b"first\xff.wav"
        );
    }

    #[test]
    fn path_count_overflow_is_captured_as_overflow() {
        let paths = (0..=MAX_CAPTURE_PATHS)
            .map(|index| PathBuf::from(format!("{index}.wav")))
            .collect();
        let event = Event {
            kind: EventKind::Any,
            paths,
            attrs: EventAttributes::default(),
        };

        assert!(matches!(
            capture_event(Ok(event)),
            SourceWatcherCapture::Overflow { .. }
        ));
    }

    #[test]
    fn path_byte_overflow_is_captured_as_overflow() {
        let path = PathBuf::from("x".repeat(MAX_CAPTURE_PATH_BYTES + 1));
        let event = Event {
            kind: EventKind::Any,
            paths: vec![path],
            attrs: EventAttributes::default(),
        };

        assert!(matches!(
            capture_event(Ok(event)),
            SourceWatcherCapture::Overflow { .. }
        ));
    }

    #[test]
    fn fsevents_replay_is_one_conservative_modify_observation() {
        let (observations, limits) = fsevents_replay_observations(vec![
            PathBuf::from("kick.wav"),
            PathBuf::from("drums/snare.wav"),
        ])
        .expect("bounded replay observation");
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].kind(), RawEventKind::Modify);
        assert_eq!(observations[0].paths().len(), 2);
        assert_eq!(observations[0].paths()[0].role(), RawPathRole::Subject);
        assert_eq!(limits.max_events(), 1);
    }

    #[test]
    fn oversized_fsevents_replay_is_rejected_before_owner_admission() {
        let paths = (0..=MAX_CAPTURE_PATHS)
            .map(|index| PathBuf::from(format!("{index}.wav")))
            .collect();
        assert!(matches!(
            fsevents_replay_observations(paths),
            Err(CaptureAdmissionError::ReplayPathCountExceeded { .. })
        ));
    }

    #[test]
    fn metadata_byte_overflow_is_captured_as_overflow() {
        let mut attrs = EventAttributes::default();
        attrs.set_info(&"é".repeat(MAX_CAPTURE_METADATA_BYTES / 2 + 1));
        let event = Event {
            kind: EventKind::Any,
            paths: Vec::new(),
            attrs,
        };

        assert!(matches!(
            capture_event(Ok(event)),
            SourceWatcherCapture::Overflow { .. }
        ));
    }

    #[test]
    fn metadata_boundary_includes_path_and_optional_raw_overhead() {
        let accepted = event_at_raw_metadata_boundary(MAX_CAPTURE_METADATA_BYTES);
        assert!(matches!(
            capture_event(Ok(accepted)),
            SourceWatcherCapture::Notify { .. }
        ));

        let accepted_batch = event_to_observation_batch(
            event_at_raw_metadata_boundary(MAX_CAPTURE_METADATA_BYTES),
            Path::new("source-root"),
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            7,
            WatcherGeneration::new(3),
            boundary(),
        )
        .expect("accepted boundary event");
        let (provenance, observations, limits) = accepted_batch.into_parts();
        let envelope = RawObservationEnvelope::try_new(provenance, observations, limits)
            .expect("boundary event fits the raw envelope");
        assert_eq!(
            envelope.accounting().metadata_bytes(),
            MAX_CAPTURE_METADATA_BYTES
        );

        let rejected = event_at_raw_metadata_boundary(MAX_CAPTURE_METADATA_BYTES + 1);
        assert!(matches!(
            capture_event(Ok(rejected)),
            SourceWatcherCapture::Overflow { .. }
        ));
    }

    #[test]
    fn notify_error_is_reduced_to_a_bounded_marker() {
        let error = notify::Error::generic(&"x".repeat(MAX_CAPTURE_METADATA_BYTES + 1))
            .set_paths(vec![PathBuf::from("error-path")]);

        assert!(matches!(
            capture_event(Err(error)),
            SourceWatcherCapture::Error { .. }
        ));
    }

    #[test]
    fn notify_kinds_map_to_conservative_raw_kinds_and_roles() {
        let root = Path::new("source-root");

        let created_folder = observation(
            event(EventKind::Create(CreateKind::Folder), root, &["folder"]),
            root,
        );
        assert_eq!(created_folder.kind(), RawEventKind::Create);
        assert_eq!(created_folder.paths()[0].role(), RawPathRole::Subject);
        assert_eq!(
            created_folder.paths()[0].hint(),
            Some(RawPathHint::Directory)
        );

        let modified = observation(
            event(
                EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                root,
                &["sample.wav"],
            ),
            root,
        );
        assert_eq!(modified.kind(), RawEventKind::Modify);
        assert_eq!(modified.paths()[0].role(), RawPathRole::Subject);

        let removed_folder = observation(
            event(EventKind::Remove(RemoveKind::Folder), root, &["folder"]),
            root,
        );
        assert_eq!(removed_folder.kind(), RawEventKind::Delete);
        assert_eq!(
            removed_folder.paths()[0].hint(),
            Some(RawPathHint::Directory)
        );

        let renamed = observation(
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                root,
                &["old.wav", "new.wav"],
            ),
            root,
        );
        assert_eq!(renamed.kind(), RawEventKind::Rename);
        assert_eq!(
            renamed
                .paths()
                .iter()
                .map(|path| path.role())
                .collect::<Vec<_>>(),
            vec![RawPathRole::RenameSource, RawPathRole::RenameDestination]
        );
        assert!(renamed.uncertainty().is_empty());

        let incomplete_rename = observation(
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                root,
                &["old.wav"],
            ),
            root,
        );
        assert_eq!(incomplete_rename.kind(), RawEventKind::Rename);
        assert_eq!(
            incomplete_rename.paths()[0].role(),
            RawPathRole::RenameSource
        );
        assert!(
            incomplete_rename
                .uncertainty()
                .contains(ObservationUncertainty::PATH_COVERAGE)
        );

        let ambiguous_rename = observation(
            event(
                EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
                root,
                &["one.wav", "two.wav"],
            ),
            root,
        );
        assert_eq!(ambiguous_rename.kind(), RawEventKind::Rename);
        assert!(
            ambiguous_rename
                .paths()
                .iter()
                .all(|path| path.role() == RawPathRole::Subject)
        );
        assert!(
            ambiguous_rename
                .uncertainty()
                .contains(ObservationUncertainty::PATH_COVERAGE)
        );

        let unsupported = observation(
            event(
                EventKind::Access(AccessKind::Open(AccessMode::Write)),
                root,
                &["sample.wav"],
            ),
            root,
        );
        assert_eq!(unsupported.kind(), RawEventKind::Unsupported);
        assert!(
            unsupported
                .uncertainty()
                .contains(ObservationUncertainty::PATH_COVERAGE)
        );

        let unsupported_modify = observation(
            event(EventKind::Modify(ModifyKind::Other), root, &["sample.wav"]),
            root,
        );
        assert_eq!(unsupported_modify.kind(), RawEventKind::Unsupported);
    }

    #[test]
    fn relative_conversion_preserves_event_order_and_duplicate_paths() {
        let root = Path::new("source-root");
        let paths = vec![
            root.join("third.wav"),
            root.join("first.wav"),
            root.join("first.wav"),
        ];
        let observed = observation(
            Event {
                kind: EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                paths,
                attrs: EventAttributes::default(),
            },
            root,
        );

        assert_eq!(
            observed
                .paths()
                .iter()
                .map(|path| path.path())
                .collect::<Vec<_>>(),
            vec![
                Path::new("third.wav"),
                Path::new("first.wav"),
                Path::new("first.wav")
            ]
        );
    }

    #[test]
    fn notify_metadata_is_retained_on_path_and_root_observations() {
        let root = Path::new("source-root");
        let mut attrs = EventAttributes::new();
        attrs.set_flag(Flag::Rescan);
        attrs.set_tracker(73);
        attrs.set_info("capture metadata");
        attrs.set_process_id(42);
        let metadata = observation_metadata_from_values(
            attrs.flag(),
            attrs.tracker(),
            attrs.info(),
            Some("notify backend"),
            attrs.process_id(),
        )
        .expect("metadata");
        let path_observation = event_observation_with_metadata(
            Event {
                kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                paths: vec![root.join("sample.wav")],
                attrs: attrs.clone(),
            },
            root,
            metadata.clone(),
            true,
        )
        .expect("path observation");
        assert_eq!(path_observation.metadata().flags(), 1);
        assert_eq!(path_observation.metadata().rename_cookie(), Some(73));
        assert_eq!(
            path_observation
                .metadata()
                .detail()
                .and_then(|detail| detail.to_str()),
            Some("capture metadata")
        );
        assert_eq!(
            path_observation
                .metadata()
                .source()
                .and_then(|source| source.to_str()),
            Some("notify backend")
        );
        assert_eq!(path_observation.metadata().process_id(), Some(42));
        assert_eq!(path_observation.paths()[0].path(), Path::new("sample.wav"));

        let root_observation = event_observation_with_metadata(
            Event {
                kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                paths: vec![root.to_path_buf()],
                attrs: attrs.clone(),
            },
            root,
            metadata,
            true,
        )
        .expect("root observation");
        assert_eq!(root_observation.kind(), RawEventKind::RootChanged);
        assert_eq!(root_observation.metadata().flags(), 1);
        assert_eq!(root_observation.metadata().rename_cookie(), Some(73));
        assert_eq!(
            root_observation
                .metadata()
                .detail()
                .and_then(|detail| detail.to_str()),
            Some("capture metadata")
        );
        assert_eq!(
            root_observation
                .metadata()
                .source()
                .and_then(|source| source.to_str()),
            Some("notify backend")
        );
        assert_eq!(root_observation.metadata().process_id(), Some(42));

        let callback_batch = event_to_observation_batch(
            Event {
                kind: EventKind::Modify(ModifyKind::Data(DataChange::Content)),
                paths: vec![root.join("sample.wav")],
                attrs,
            },
            root,
            SourceId::from_string("capture-metadata-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            7,
            WatcherGeneration::new(3),
            boundary(),
        )
        .expect("callback observation");
        let callback_observation = &callback_batch.observations()[0];
        assert_eq!(callback_observation.metadata().flags(), 1);
        assert_eq!(callback_observation.metadata().rename_cookie(), Some(73));
        assert_eq!(
            callback_observation
                .metadata()
                .detail()
                .and_then(|detail| detail.to_str()),
            Some("capture metadata")
        );
        assert_eq!(callback_observation.metadata().process_id(), Some(42));
        assert_eq!(
            callback_batch
                .provenance()
                .capture_boundary()
                .first_sequence(),
            None
        );
        assert_eq!(
            callback_batch
                .provenance()
                .capture_boundary()
                .last_sequence(),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn relative_conversion_preserves_non_utf8_path_bytes() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let root = Path::new("source-root");
        let name = OsString::from_vec(vec![b'c', b'u', b't', 0xff, b'.', b'w', b'a', b'v']);
        let mut path = root.to_path_buf();
        path.push(&name);
        let observed = observation(
            Event {
                kind: EventKind::Create(CreateKind::File),
                paths: vec![path],
                attrs: EventAttributes::default(),
            },
            root,
        );

        assert_eq!(
            observed.paths()[0].path().as_os_str().as_encoded_bytes(),
            b"cut\xff.wav"
        );
    }

    #[test]
    fn paths_outside_or_traversing_the_source_root_are_rejected() {
        let root = Path::new("source-root");
        let outside = PathBuf::from("other-root/sample.wav");
        let outside_result = event_to_observation_batch(
            Event {
                kind: EventKind::Create(CreateKind::File),
                paths: vec![outside.clone()],
                attrs: EventAttributes::default(),
            },
            root,
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            7,
            WatcherGeneration::new(3),
            boundary(),
        );
        assert_eq!(
            outside_result,
            Err(CaptureAdmissionError::PathOutsideSourceRoot { path: outside })
        );

        let traversal = root.join("../escape.wav");
        let traversal_result = event_to_observation_batch(
            Event {
                kind: EventKind::Create(CreateKind::File),
                paths: vec![traversal.clone()],
                attrs: EventAttributes::default(),
            },
            root,
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            7,
            WatcherGeneration::new(3),
            boundary(),
        );
        assert_eq!(
            traversal_result,
            Err(CaptureAdmissionError::InvalidRelativePath {
                path: traversal,
                error: RootRelativePathError::ParentTraversal,
            })
        );
    }

    #[test]
    fn overflow_and_error_captures_become_bounded_uncertainty_markers() {
        let root = Path::new("source-root");
        let overflow = capture_to_observation_batch(
            SourceWatcherCapture::Overflow { stream_id: 41 },
            root,
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            WatcherGeneration::new(3),
            boundary(),
        )
        .expect("overflow marker batch");
        assert_eq!(overflow.observations()[0].kind(), RawEventKind::Overflow);
        assert!(
            overflow.observations()[0]
                .uncertainty()
                .contains(ObservationUncertainty::OVERFLOW)
        );
        assert!(overflow.observations()[0].paths().is_empty());

        let error = capture_to_observation_batch(
            SourceWatcherCapture::Error { stream_id: 41 },
            root,
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            WatcherGeneration::new(3),
            boundary(),
        )
        .expect("error marker batch");
        assert_eq!(error.observations()[0].kind(), RawEventKind::Error);
        assert!(
            error.observations()[0]
                .uncertainty()
                .contains(ObservationUncertainty::BACKEND_ERROR)
        );
        assert!(error.observations()[0].paths().is_empty());

        let oversized = Event {
            kind: EventKind::Any,
            paths: (0..=MAX_CAPTURE_PATHS)
                .map(|index| root.join(format!("{index}.wav")))
                .collect(),
            attrs: EventAttributes::default(),
        };
        let oversized_batch = event_to_observation_batch(
            oversized,
            root,
            SourceId::from_string("capture-test-source"),
            RootIdentity::from_bytes(b"root".to_vec()),
            41,
            WatcherGeneration::new(3),
            boundary(),
        )
        .expect("oversized event marker batch");
        assert_eq!(
            oversized_batch.observations()[0].kind(),
            RawEventKind::Overflow
        );
    }

    #[test]
    fn batch_provenance_retains_supplied_identity_and_capture_boundary() {
        let root = Path::new("source-root");
        let capture_boundary =
            CaptureBoundary::try_new(9_001, Some(12), Some(13)).expect("capture boundary");
        let stream_id = 0x0102_0304_0506_0708;
        let batch = event_to_observation_batch(
            event(EventKind::Create(CreateKind::File), root, &["sample.wav"]),
            root,
            SourceId::from_string("exact-source"),
            RootIdentity::from_bytes(vec![0, 0xff, 7]),
            stream_id,
            WatcherGeneration::new(44),
            capture_boundary,
        )
        .expect("provenance batch");
        let provenance = batch.provenance();

        assert_eq!(provenance.source_id().as_str(), "exact-source");
        assert_eq!(
            provenance
                .root_identity()
                .expect("root identity")
                .as_bytes(),
            &[0, 0xff, 7]
        );
        assert_eq!(
            provenance
                .backend_stream_identity()
                .expect("stream identity")
                .as_bytes(),
            &stream_id.to_be_bytes()
        );
        assert_eq!(provenance.watcher_generation(), WatcherGeneration::new(44));
        assert_eq!(provenance.capture_boundary(), capture_boundary);
        assert_eq!(capture_boundary.captured_at(), 9_001);
        assert_eq!(capture_boundary.first_sequence(), Some(12));
        assert_eq!(capture_boundary.last_sequence(), Some(13));
    }
}
