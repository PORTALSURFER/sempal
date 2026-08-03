use notify::Event;

pub(super) const MAX_CAPTURE_PATHS: usize = 4_096;
pub(super) const MAX_CAPTURE_PATH_BYTES: usize = 256 * 1_024;
pub(super) const MAX_CAPTURE_METADATA_BYTES: usize = 256 * 1_024;

pub(super) enum SourceWatcherCapture {
    Notify { stream_id: u64, event: Event },
    Error,
    Overflow,
}

pub(super) fn capture_event(event: notify::Result<Event>) -> SourceWatcherCapture {
    let Ok(captured_event) = event else {
        return SourceWatcherCapture::Error;
    };

    if captured_event.paths.len() > MAX_CAPTURE_PATHS {
        return SourceWatcherCapture::Overflow;
    }

    let path_bytes = captured_event.paths.iter().try_fold(0usize, |total, path| {
        let total = total.checked_add(path.as_os_str().as_encoded_bytes().len())?;
        (total <= MAX_CAPTURE_PATH_BYTES).then_some(total)
    });
    let metadata_bytes = captured_event
        .attrs
        .info()
        .into_iter()
        .chain(captured_event.attrs.source())
        .try_fold(0usize, |total, metadata| {
            total.checked_add(metadata.as_bytes().len())
        });
    if path_bytes.is_none()
        || metadata_bytes.map_or(true, |bytes| bytes > MAX_CAPTURE_METADATA_BYTES)
    {
        SourceWatcherCapture::Overflow
    } else {
        SourceWatcherCapture::Notify {
            stream_id: 0,
            event: captured_event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CAPTURE_METADATA_BYTES, MAX_CAPTURE_PATH_BYTES, MAX_CAPTURE_PATHS,
        SourceWatcherCapture, capture_event,
    };
    use notify::event::EventAttributes;
    use notify::{Event, EventKind};
    use std::path::PathBuf;

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
            SourceWatcherCapture::Overflow
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
            SourceWatcherCapture::Overflow
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
            SourceWatcherCapture::Overflow
        ));
    }

    #[test]
    fn notify_error_is_reduced_to_a_bounded_marker() {
        let error = notify::Error::generic(&"x".repeat(MAX_CAPTURE_METADATA_BYTES + 1))
            .set_paths(vec![PathBuf::from("error-path")]);

        assert!(matches!(
            capture_event(Err(error)),
            SourceWatcherCapture::Error
        ));
    }
}
