//! Deterministic sample entries written into controller GUI fixtures.

use crate::sample_sources::{Rating, WavEntry};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::{
    fs,
    path::{Path, PathBuf},
};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

const BROWSER_FIXTURE_ENTRY_COUNT: usize = 40;

pub(super) fn browser_fixture_entries(root: &Path) -> Result<Vec<WavEntry>, String> {
    let mut entries = Vec::with_capacity(BROWSER_FIXTURE_ENTRY_COUNT);
    for index in 0..BROWSER_FIXTURE_ENTRY_COUNT {
        let (name, tag) = browser_fixture_entry_spec(index);
        let samples = browser_fixture_samples(index);
        entries.push(write_fixture_entry(root, &name, &samples, tag)?);
    }
    Ok(entries)
}

fn browser_fixture_entry_spec(index: usize) -> (String, Rating) {
    if let Some((name, tag)) = fixed_browser_fixture_entry(index) {
        return (String::from(name), tag);
    }
    let name = format!("{}_{index:02}.wav", generated_browser_entry_prefix(index));
    (name, generated_browser_entry_rating(index))
}

fn fixed_browser_fixture_entry(index: usize) -> Option<(&'static str, Rating)> {
    match index {
        0 => Some(("kick_one.wav", Rating::NEUTRAL)),
        1 => Some(("snare_two.wav", Rating::KEEP_3)),
        2 => Some(("hat_three.wav", Rating::TRASH_1)),
        3 => Some(("loop_four.wav", Rating::KEEP_1)),
        4 => Some(("fx_five.wav", Rating::TRASH_3)),
        _ => None,
    }
}

fn generated_browser_entry_prefix(index: usize) -> &'static str {
    match index % 8 {
        0 => "kick",
        1 => "snare",
        2 => "hat",
        3 => "loop",
        4 => "fx",
        5 => "bass",
        6 => "perc",
        _ => "stab",
    }
}

fn generated_browser_entry_rating(index: usize) -> Rating {
    match index % 5 {
        0 => Rating::new(-2),
        1 => Rating::NEUTRAL,
        2 => Rating::KEEP_1,
        3 => Rating::new(2),
        _ => Rating::TRASH_1,
    }
}

fn browser_fixture_samples(index: usize) -> Vec<f32> {
    let seed = index as f32 + 1.0;
    vec![
        0.0125 * seed,
        0.18 + ((index % 5) as f32 * 0.05),
        -0.10 - ((index % 4) as f32 * 0.04),
        0.06 * ((index % 3) as f32 + 1.0),
        -0.03 * ((index % 6) as f32 + 1.0),
        0.015 * ((index % 7) as f32 + 1.0),
    ]
}

pub(super) fn dense_waveform_fixture_samples() -> Vec<f32> {
    let sample_count = 4096;
    (0..sample_count)
        .map(|index| {
            let phase = index as f32 / 18.0;
            let contour = ((index as f32 / sample_count as f32) * std::f32::consts::PI)
                .sin()
                .max(0.18);
            (phase.sin() * 0.62 * contour).clamp(-0.95, 0.95)
        })
        .collect()
}

pub(super) fn write_fixture_entry(
    root: &Path,
    name: &str,
    samples: &[f32],
    tag: Rating,
) -> Result<WavEntry, String> {
    let path = root.join(name);
    write_fixture_wav(&path, samples)?;
    wav_entry_from_file(&path, name, tag)
}

fn write_fixture_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)
        .map_err(|err| format!("create fixture wav {}: {err}", path.display()))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("write fixture wav {}: {err}", path.display()))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("finalize fixture wav {}: {err}", path.display()))
}

fn wav_entry_from_file(path: &Path, name: &str, tag: Rating) -> Result<WavEntry, String> {
    let metadata = fs::metadata(path)
        .map_err(|err| format!("read fixture wav metadata {}: {err}", path.display()))?;
    let modified_ns = fixture_modified_ns(path, &metadata)?;
    Ok(WavEntry {
        relative_path: PathBuf::from(name),
        file_size: metadata.len(),
        modified_ns,
        content_hash: Some(format!("fixture-{name}")),
        tag,
        looped: false,
        sound_type: None,
        locked: false,
        missing: false,
        last_played_at: None,
        last_curated_at: None,
        user_tag: None,
        tag_named: false,
        normal_tags: Vec::new(),
    })
}

fn fixture_modified_ns(path: &Path, metadata: &fs::Metadata) -> Result<i64, String> {
    let modified = metadata
        .modified()
        .map_err(|err| format!("read fixture wav modified time {}: {err}", path.display()))?;
    Ok(system_time_to_unix_nanos(modified))
}
