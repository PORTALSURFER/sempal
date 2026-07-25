#[cfg(not(test))]
use std::sync::OnceLock;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::native_app::app::{NativeAppState, WaveformState};

const SAMPLE_IDENTITY_HASH_CHUNK: usize = 4096;
const SAMPLE_IDENTITY_HASH_F32_CHUNK: usize = 64;
#[cfg(not(test))]
const SAMPLE_IDENTITY_DIAGNOSTICS_ENV: &str = "WAVECRATE_SAMPLE_IDENTITY_DIAGNOSTICS";

#[cfg(not(test))]
static SAMPLE_IDENTITY_DIAGNOSTICS_ENABLED: OnceLock<bool> = OnceLock::new();

impl NativeAppState {
    pub(in crate::native_app) fn log_sample_identity_checkpoint(
        &self,
        event: &'static str,
        trigger: &'static str,
        requested_path: Option<&Path>,
        note: Option<&str>,
    ) {
        if !sample_identity_info_enabled() {
            return;
        }
        let selected_file_ids = self
            .library
            .folder_browser
            .selected_file_ids_for_diagnostics();
        let active_file_ids = self
            .library
            .folder_browser
            .active_file_ids_for_diagnostics();
        let normalizing_paths = sorted_path_strings(
            self.background
                .normalization_active_paths
                .iter()
                .map(|path| path.as_path()),
        );
        let loaded_path = self.waveform.current.path();
        let requested_path_fingerprint = requested_path.map(path_fingerprint);
        let loaded_waveform_fingerprint = waveform_fingerprint(&self.waveform.current);
        tracing::info!(
            target: "wavecrate::debug::sample_identity",
            event,
            trigger,
            requested_path = requested_path.map(|path| path.display().to_string()).as_deref(),
            selected_file_id = self.library.folder_browser.selected_file_id(),
            selected_file_ids = ?selected_file_ids,
            active_file_ids = ?active_file_ids,
            selected_file_count = selected_file_ids.len(),
            selected_file_ids_explicit = self
                .library
                .folder_browser
                .selected_file_ids_explicit_for_diagnostics(),
            selected_source_id = self.library.folder_browser.selected_source_id(),
            loaded_waveform_path = %loaded_path.display(),
            loaded_waveform_has_sample = self.waveform.current.has_loaded_sample(),
            load_selection_path = self.waveform.load.selection.selected_path.as_deref(),
            active_sample_load = self.active_sample_load_task().is_some(),
            deferred_sample_load = self.background.deferred_sample_load_task.active().is_some(),
            sample_load_validation = self.background.sample_load_validation_task.active().is_some(),
            pending_sample_playback = ?self.audio.pending_sample_playback,
            normalizing_paths = ?normalizing_paths,
            requested_path_fingerprint = ?requested_path_fingerprint,
            loaded_waveform_fingerprint = ?loaded_waveform_fingerprint,
            note,
            "Sample identity checkpoint"
        );
    }

    pub(in crate::native_app) fn log_sample_identity_paths_checkpoint(
        &self,
        event: &'static str,
        trigger: &'static str,
        requested_paths: &[PathBuf],
        note: Option<&str>,
    ) {
        if !sample_identity_info_enabled() {
            return;
        }
        let selected_file_ids = self
            .library
            .folder_browser
            .selected_file_ids_for_diagnostics();
        let active_file_ids = self
            .library
            .folder_browser
            .active_file_ids_for_diagnostics();
        let requested_path_fingerprints = requested_paths
            .iter()
            .map(|path| (path.display().to_string(), path_fingerprint(path)))
            .collect::<Vec<_>>();
        let requested_paths =
            sorted_path_strings(requested_paths.iter().map(|path| path.as_path()));
        let normalizing_paths = sorted_path_strings(
            self.background
                .normalization_active_paths
                .iter()
                .map(|path| path.as_path()),
        );
        let loaded_path = self.waveform.current.path();
        let loaded_waveform_fingerprint = waveform_fingerprint(&self.waveform.current);
        tracing::info!(
            target: "wavecrate::debug::sample_identity",
            event,
            trigger,
            requested_paths = ?requested_paths,
            selected_file_id = self.library.folder_browser.selected_file_id(),
            selected_file_ids = ?selected_file_ids,
            active_file_ids = ?active_file_ids,
            selected_file_count = selected_file_ids.len(),
            selected_file_ids_explicit = self
                .library
                .folder_browser
                .selected_file_ids_explicit_for_diagnostics(),
            selected_source_id = self.library.folder_browser.selected_source_id(),
            loaded_waveform_path = %loaded_path.display(),
            loaded_waveform_has_sample = self.waveform.current.has_loaded_sample(),
            load_selection_path = self.waveform.load.selection.selected_path.as_deref(),
            active_sample_load = self.active_sample_load_task().is_some(),
            deferred_sample_load = self.background.deferred_sample_load_task.active().is_some(),
            sample_load_validation = self.background.sample_load_validation_task.active().is_some(),
            pending_sample_playback = ?self.audio.pending_sample_playback,
            normalizing_paths = ?normalizing_paths,
            requested_path_fingerprints = ?requested_path_fingerprints,
            loaded_waveform_fingerprint = ?loaded_waveform_fingerprint,
            note,
            "Sample identity checkpoint"
        );
    }

    pub(in crate::native_app) fn log_sample_identity_waveform_checkpoint(
        &self,
        event: &'static str,
        trigger: &'static str,
        requested_path: Option<&Path>,
        waveform: &WaveformState,
        note: Option<&str>,
    ) {
        if !sample_identity_info_enabled() {
            return;
        }
        let selected_file_ids = self
            .library
            .folder_browser
            .selected_file_ids_for_diagnostics();
        let active_file_ids = self
            .library
            .folder_browser
            .active_file_ids_for_diagnostics();
        tracing::info!(
            target: "wavecrate::debug::sample_identity",
            event,
            trigger,
            requested_path = requested_path.map(|path| path.display().to_string()).as_deref(),
            selected_file_id = self.library.folder_browser.selected_file_id(),
            selected_file_ids = ?selected_file_ids,
            active_file_ids = ?active_file_ids,
            selected_source_id = self.library.folder_browser.selected_source_id(),
            loaded_waveform_fingerprint = ?waveform_fingerprint(&self.waveform.current),
            candidate_waveform_fingerprint = ?waveform_fingerprint(waveform),
            requested_path_fingerprint = ?requested_path.map(path_fingerprint),
            note,
            "Sample identity waveform checkpoint"
        );
    }
}

pub(in crate::native_app) fn log_sample_identity_waveform_result(
    event: &'static str,
    trigger: &'static str,
    requested_path: &Path,
    result: &Result<WaveformState, String>,
    note: Option<&str>,
) {
    if !sample_identity_info_enabled() {
        return;
    }
    match result {
        Ok(waveform) => {
            tracing::info!(
            target: "wavecrate::debug::sample_identity",
                event,
                trigger,
                requested_path = %requested_path.display(),
                requested_path_fingerprint = ?path_fingerprint(requested_path),
                result_waveform_fingerprint = ?waveform_fingerprint(waveform),
                note,
                "Sample identity waveform result"
            );
        }
        Err(error) => {
            tracing::info!(
            target: "wavecrate::debug::sample_identity",
                event,
                trigger,
                requested_path = %requested_path.display(),
                requested_path_fingerprint = ?path_fingerprint(requested_path),
                error,
                note,
                "Sample identity waveform result"
            );
        }
    }
}

pub(in crate::native_app) fn log_sample_identity_path_event(
    event: &'static str,
    trigger: &'static str,
    path: &Path,
    note: Option<&str>,
) {
    if !sample_identity_info_enabled() {
        return;
    }
    tracing::info!(
            target: "wavecrate::debug::sample_identity",
        event,
        trigger,
        path = %path.display(),
        path_fingerprint = ?path_fingerprint(path),
        note,
        "Sample identity path event"
    );
}

fn sample_identity_info_enabled() -> bool {
    wavecrate::logging::debug_logging_enabled()
        && sample_identity_diagnostics_enabled()
        && tracing::enabled!(target: "wavecrate::debug::sample_identity", tracing::Level::INFO)
}

fn sample_identity_diagnostics_enabled() -> bool {
    #[cfg(test)]
    {
        false
    }
    #[cfg(not(test))]
    *SAMPLE_IDENTITY_DIAGNOSTICS_ENABLED
        .get_or_init(|| env_var_truthy(SAMPLE_IDENTITY_DIAGNOSTICS_ENV))
}

#[cfg(not(test))]
fn env_var_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn sorted_path_strings<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<String> {
    let mut paths = paths
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[derive(Debug)]
#[allow(dead_code)]
struct PathFingerprint {
    exists: bool,
    len: Option<u64>,
    modified_unix_ms: Option<u128>,
    sampled: Option<SampledByteFingerprint>,
    error: Option<String>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct WaveformFingerprint {
    path: String,
    file: PathFingerprint,
    sample_rate: u32,
    channels: usize,
    frames: usize,
    duration_seconds: f32,
    audio_bytes: Option<SampledByteFingerprint>,
    playback_samples: Option<SampledF32Fingerprint>,
    playback_cache_file: Option<PlaybackCacheFingerprint>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct PlaybackCacheFingerprint {
    path: String,
    sample_count: u64,
    sampled: Option<SampledByteFingerprint>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct SampledByteFingerprint {
    len: usize,
    hash: String,
    first: String,
    last: String,
}

#[derive(Debug)]
#[allow(dead_code)]
struct SampledF32Fingerprint {
    len: usize,
    hash: String,
    first_bits: Vec<u32>,
    last_bits: Vec<u32>,
}

fn waveform_fingerprint(waveform: &WaveformState) -> WaveformFingerprint {
    let path = waveform.path();
    let playback_cache_file =
        waveform
            .playback_cache_file()
            .map(|cache_file| PlaybackCacheFingerprint {
                path: cache_file.path.display().to_string(),
                sample_count: cache_file.sample_count,
                sampled: sampled_file_fingerprint(&cache_file.path).ok().flatten(),
            });
    WaveformFingerprint {
        path: path.display().to_string(),
        file: path_fingerprint(&path),
        sample_rate: waveform.sample_rate(),
        channels: waveform.channels(),
        frames: waveform.frames(),
        duration_seconds: waveform.duration_seconds(),
        audio_bytes: sampled_byte_fingerprint(&waveform.audio_bytes()),
        playback_samples: waveform
            .playback_samples()
            .as_deref()
            .and_then(sampled_f32_fingerprint),
        playback_cache_file,
    }
}

fn path_fingerprint(path: &Path) -> PathFingerprint {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return PathFingerprint {
                exists: false,
                len: None,
                modified_unix_ms: None,
                sampled: None,
                error: Some(error.to_string()),
            };
        }
    };
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    let sampled = match sampled_file_fingerprint(path) {
        Ok(sampled) => sampled,
        Err(error) => {
            return PathFingerprint {
                exists: true,
                len: Some(metadata.len()),
                modified_unix_ms,
                sampled: None,
                error: Some(error),
            };
        }
    };
    PathFingerprint {
        exists: true,
        len: Some(metadata.len()),
        modified_unix_ms,
        sampled,
        error: None,
    }
}

fn sampled_file_fingerprint(path: &Path) -> Result<Option<SampledByteFingerprint>, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let len = file
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        .try_into()
        .unwrap_or(usize::MAX);
    if len == 0 {
        return Ok(None);
    }
    if len <= SAMPLE_IDENTITY_HASH_CHUNK * 3 {
        let mut bytes = Vec::with_capacity(len);
        file.read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        return Ok(sampled_byte_fingerprint(&bytes));
    }

    let mut sampled = Vec::with_capacity(SAMPLE_IDENTITY_HASH_CHUNK * 3);
    read_file_chunk(&mut file, 0, &mut sampled)?;
    read_file_chunk(
        &mut file,
        (len / 2).saturating_sub(SAMPLE_IDENTITY_HASH_CHUNK / 2) as u64,
        &mut sampled,
    )?;
    read_file_chunk(
        &mut file,
        len.saturating_sub(SAMPLE_IDENTITY_HASH_CHUNK) as u64,
        &mut sampled,
    )?;
    Ok(sampled_byte_fingerprint_with_len(len, &sampled))
}

fn read_file_chunk(file: &mut File, offset: u64, sampled: &mut Vec<u8>) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut chunk = vec![0_u8; SAMPLE_IDENTITY_HASH_CHUNK];
    let read = file.read(&mut chunk).map_err(|error| error.to_string())?;
    sampled.extend_from_slice(&chunk[..read]);
    Ok(())
}

fn sampled_byte_fingerprint(bytes: &[u8]) -> Option<SampledByteFingerprint> {
    sampled_byte_fingerprint_with_len(bytes.len(), bytes)
}

fn sampled_byte_fingerprint_with_len(len: usize, sampled: &[u8]) -> Option<SampledByteFingerprint> {
    if len == 0 {
        return None;
    }
    let first_len = sampled.len().min(8);
    let last_start = sampled.len().saturating_sub(8);
    Some(SampledByteFingerprint {
        len,
        hash: format!("{:016x}", stable_bytes_hash(len, sampled)),
        first: hex_bytes(&sampled[..first_len]),
        last: hex_bytes(&sampled[last_start..]),
    })
}

fn sampled_f32_fingerprint(samples: &[f32]) -> Option<SampledF32Fingerprint> {
    if samples.is_empty() {
        return None;
    }
    let mut bits = Vec::new();
    bits.extend(
        samples
            .iter()
            .take(SAMPLE_IDENTITY_HASH_F32_CHUNK)
            .map(|sample| sample.to_bits()),
    );
    let middle_start = samples
        .len()
        .saturating_div(2)
        .saturating_sub(SAMPLE_IDENTITY_HASH_F32_CHUNK / 2);
    bits.extend(
        samples
            .iter()
            .skip(middle_start)
            .take(SAMPLE_IDENTITY_HASH_F32_CHUNK)
            .map(|sample| sample.to_bits()),
    );
    let tail_start = samples.len().saturating_sub(SAMPLE_IDENTITY_HASH_F32_CHUNK);
    bits.extend(
        samples
            .iter()
            .skip(tail_start)
            .map(|sample| sample.to_bits()),
    );

    Some(SampledF32Fingerprint {
        len: samples.len(),
        hash: format!("{:016x}", stable_u32_hash(samples.len(), &bits)),
        first_bits: samples
            .iter()
            .take(4)
            .map(|sample| sample.to_bits())
            .collect(),
        last_bits: samples
            .iter()
            .skip(samples.len().saturating_sub(4))
            .map(|sample| sample.to_bits())
            .collect(),
    })
}

fn stable_bytes_hash(len: usize, bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ len as u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn stable_u32_hash(len: usize, values: &[u32]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ len as u64;
    for value in values {
        for byte in value.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
