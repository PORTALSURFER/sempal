//! File I/O helpers for waveform operations.

use hound::SampleFormat;
use std::path::Path;
use wavecrate_library::timestamps::system_time_to_unix_nanos;

/// Return whether the given path is a WAV file supported by destructive edit flows.
pub(crate) fn supports_wav_destructive_edits(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
}

/// Reject non-WAV targets with a deliberate browser-facing error contract.
pub(crate) fn ensure_wav_destructive_edit_target(
    path: &Path,
    action_label: &str,
) -> Result<(), String> {
    if supports_wav_destructive_edits(path) {
        return Ok(());
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_else(|| "this file type".to_string());
    Err(format!(
        "{action_label} only supports WAV files; {extension} is not supported"
    ))
}

/// Reject multichannel WAV files for ordinary destructive edit flows.
pub(crate) fn ensure_mono_stereo_wav_destructive_edit_target(
    path: &Path,
    action_label: &str,
) -> Result<hound::WavSpec, String> {
    ensure_wav_destructive_edit_target(path, action_label)?;
    let reader_source = crate::wav_sanitize::open_sanitized_wav(path)?;
    let buf_reader = std::io::BufReader::with_capacity(1024 * 1024, reader_source);
    let reader = hound::WavReader::new(buf_reader).map_err(|err| format!("Invalid wav: {err}"))?;
    let spec = reader.spec();
    if spec.channels > 2 {
        return Err(format!(
            "{action_label} only supports mono or stereo WAV files; {} channels are not currently editable",
            spec.channels
        ));
    }
    Ok(spec)
}

/// Read WAV samples for normalization workflows.
pub(crate) fn read_samples_for_normalization(
    path: &Path,
) -> Result<(Vec<f32>, hound::WavSpec), String> {
    ensure_wav_destructive_edit_target(path, "This edit")?;
    let reader_source = crate::wav_sanitize::open_sanitized_wav(path)?;
    let buf_reader = std::io::BufReader::with_capacity(1024 * 1024, reader_source);
    let mut reader =
        hound::WavReader::new(buf_reader).map_err(|err| format!("Invalid wav: {err}"))?;
    let spec = reader.spec();
    if spec.channels > 2 {
        return Err(format!(
            "This edit only supports mono or stereo WAV files; {} channels are not currently editable",
            spec.channels
        ));
    }
    let samples = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|err| format!("Sample error: {err}")))
            .collect::<Result<Vec<_>, _>>()?,
        SampleFormat::Int => {
            let scale = (1i64 << spec.bits_per_sample.saturating_sub(1)).max(1) as f32;
            reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|value| value as f32 / scale)
                        .map_err(|err| format!("Sample error: {err}"))
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    Ok((samples, spec))
}

/// Write normalized WAV samples back to disk.
pub(crate) fn write_normalized_wav(
    path: &Path,
    samples: &[f32],
    spec: hound::WavSpec,
) -> Result<(), String> {
    let file =
        std::fs::File::create(path).map_err(|err| format!("Failed to create file: {err}"))?;
    let buf_writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
    let mut writer = hound::WavWriter::new(buf_writer, spec)
        .map_err(|err| format!("Failed to write wav: {err}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("Failed to write sample: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize wav: {err}"))
}

/// Fetch file size and last-modified time (epoch nanoseconds) for a path.
pub(crate) fn file_metadata(path: &Path) -> Result<(u64, i64), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let modified_ns = system_time_to_unix_nanos(
        metadata
            .modified()
            .map_err(|err| format!("Missing modified time for {}: {err}", path.display()))?,
    );
    Ok((metadata.len(), modified_ns))
}
