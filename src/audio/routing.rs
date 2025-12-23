use std::time::Duration;

pub(crate) fn normalized_progress(
    span: Option<(f32, f32)>,
    duration: f32,
    elapsed: f32,
    looping: bool,
) -> Option<f32> {
    if duration <= 0.0 {
        return None;
    }
    let (start, end) = span.unwrap_or((0.0, duration));
    let span_length = (end - start).max(f32::EPSILON);
    let within_span = if looping {
        elapsed % span_length
    } else {
        elapsed.min(span_length)
    };
    let absolute = start + within_span;
    Some((absolute / duration).clamp(0.0, 1.0))
}

pub(crate) fn duration_from_secs_f32(seconds: f32) -> Duration {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(seconds as f64)
}

pub(crate) fn duration_mod(value: Duration, modulus: Duration) -> Duration {
    let modulus_nanos = modulus.as_nanos();
    if modulus_nanos == 0 {
        return Duration::ZERO;
    }
    let remainder = value.as_nanos() % modulus_nanos;
    let secs = (remainder / 1_000_000_000) as u64;
    let nanos = (remainder % 1_000_000_000) as u32;
    Duration::new(secs, nanos)
}
