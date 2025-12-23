use super::super::normalized_progress;

#[test]
fn normalized_progress_respects_span() {
    let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 1.0, false);
    assert_eq!(progress, Some(0.3));
}

#[test]
fn normalized_progress_handles_elapsed_beyond_span() {
    let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 3.5, false);
    assert_eq!(progress, Some(0.4));
}

#[test]
fn normalized_progress_loops_within_range() {
    let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 5.5, true);
    assert!((progress.unwrap() - 0.35).abs() < 0.0001);
}

#[test]
fn normalized_progress_handles_full_track() {
    let progress = normalized_progress(None, 8.0, 3.0, false);
    assert_eq!(progress, Some(0.375));
}

#[test]
fn normalized_progress_returns_none_when_invalid_duration() {
    assert_eq!(normalized_progress(None, 0.0, 1.0, false), None);
    assert_eq!(normalized_progress(None, -1.0, 1.0, false), None);
}

#[test]
fn normalized_progress_wraps_partial_selection_when_looping() {
    let duration = 1.6;
    let span = (0.3, 1.1);
    let elapsed = (span.1 - span.0) * 1.4;

    let progress = normalized_progress(Some(span), duration, elapsed, true).unwrap();
    let expected = (span.0 + (elapsed % (span.1 - span.0))) / duration;
    assert!((progress - expected).abs() < 0.001);
}
