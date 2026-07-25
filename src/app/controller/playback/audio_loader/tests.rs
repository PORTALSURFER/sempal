use super::{AudioLoadJob, drain_to_latest_job, is_stale_request};
use crate::app::controller::playback::audio_cache::FileMetadata;
use crate::app::controller::playback::persistent_waveform_cache::persist_waveform_cache_entry;
use crate::app_dirs::ConfigBaseGuard;
use crate::waveform::DecodedWaveform;
use crate::waveform::WaveformRenderer;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::Cursor;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::{NamedTempFile, tempdir};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

mod cache_and_stretch;
mod decode_error;
mod path_safety;
mod stale_read;
mod transient_result;

fn render_spec()
-> crate::app::controller::library::wavs::waveform_rendering::InitialWaveformRenderSpec {
    crate::app::controller::library::wavs::waveform_rendering::InitialWaveformRenderSpec {
        size: [16, 16],
        channel_view: crate::waveform::WaveformChannelView::Mono,
        transient_markers_enabled: true,
    }
}

fn test_job(request_id: u64, relative_path: &str) -> AudioLoadJob {
    AudioLoadJob {
        request_id,
        source_id: crate::sample_sources::SourceId::from_string("source"),
        root: PathBuf::from("/tmp"),
        relative_path: PathBuf::from(relative_path),
        stretch_ratio: None,
        render_spec: render_spec(),
        prepared: None,
    }
}

fn test_job_with_root(
    request_id: u64,
    root: &Path,
    relative_path: &Path,
    stretch_ratio: Option<f64>,
) -> AudioLoadJob {
    AudioLoadJob {
        request_id,
        source_id: crate::sample_sources::SourceId::from_string("source"),
        root: root.to_path_buf(),
        relative_path: relative_path.to_path_buf(),
        stretch_ratio,
        render_spec: render_spec(),
        prepared: None,
    }
}

fn generated_audio_sample(frame: usize) -> f32 {
    let base = (((frame * 17) % 193) as f32 / 96.0) - 1.0;
    let accent = if frame.is_multiple_of(1024) {
        1.0
    } else if frame % 1024 == 128 {
        -1.0
    } else {
        0.0
    };
    (base * 0.42) + accent
}

fn build_float_wav(samples: &[f32], channels: u16, sample_rate: u32) -> Vec<u8> {
    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec).expect("create wav writer");
        for &sample in samples {
            writer.write_sample(sample).expect("write sample");
        }
        writer.finalize().expect("finalize wav");
    }
    cursor.into_inner()
}

fn write_test_wav(bytes: &[u8]) -> NamedTempFile {
    let mut temp = NamedTempFile::new().expect("tempfile");
    temp.write_all(bytes).expect("write wav");
    temp
}

fn decode_test_waveform(renderer: &WaveformRenderer, bytes: &[u8]) -> Arc<DecodedWaveform> {
    Arc::new(
        renderer
            .decode_from_bytes(bytes)
            .expect("decode reference waveform"),
    )
}

fn test_metadata(byte_len: usize) -> FileMetadata {
    FileMetadata {
        file_size: byte_len as u64,
        modified_ns: 123,
    }
}

fn metadata_for_path(path: &Path) -> FileMetadata {
    let metadata = std::fs::metadata(path).expect("file metadata");
    let modified_ns = system_time_to_unix_nanos(metadata.modified().expect("modified time"));
    FileMetadata {
        file_size: metadata.len(),
        modified_ns,
    }
}
