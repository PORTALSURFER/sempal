//! Background-safe duplicate and whole-file harvest file operations.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::sample_sources::{HarvestDerivationOperation, SourceDatabase};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

use super::harvest_file_ops;

#[derive(Clone, Debug)]
pub struct DuplicateSameRequest {
    pub source_path: PathBuf,
    pub source_root: PathBuf,
    pub source_database_root: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextSampleSameResult {
    pub destination: PathBuf,
}

#[derive(Clone, Debug)]
pub struct DuplicateDoubleRequest {
    pub source_path: PathBuf,
    pub target_folder: PathBuf,
    pub target_source_root: PathBuf,
    pub target_database_root: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextSampleDoubleResult {
    pub destination: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeFileHarvestExtractionResult {
    pub copied: Vec<WholeFileHarvestExtractionCopy>,
    pub failed: Vec<WholeFileHarvestExtractionFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeFileHarvestExtractionPlan {
    pub source_path: PathBuf,
    pub target_folder: PathBuf,
    pub operation: HarvestDerivationOperation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeFileHarvestExtractionCopy {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub operation: HarvestDerivationOperation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeFileHarvestExtractionFailure {
    pub source_path: PathBuf,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WholeFileHarvestExtractionRequest {
    pub copies: Vec<WholeFileHarvestExtractionPlan>,
}

pub fn execute_duplicate_context_sample_same(
    request: DuplicateSameRequest,
) -> Result<ContextSampleSameResult, String> {
    if !harvest_file_ops::path_is_file(&request.source_path) {
        return Err(String::from("Sample file is missing"));
    }
    let destination = duplicate_same_destination(&request.source_path);
    duplicate_sample_file_with_metadata(
        &request.source_path,
        &destination,
        &request.source_root,
        &request.source_database_root,
    )?;
    Ok(ContextSampleSameResult { destination })
}

pub fn execute_duplicate_context_sample_double(
    request: DuplicateDoubleRequest,
) -> Result<ContextSampleDoubleResult, String> {
    harvest_file_ops::ensure_dir(
        &request.target_folder,
        "Create duplicate destination failed",
    )?;
    let destination = next_doubled_destination(&request.source_path, &request.target_folder)?;
    let result = write_doubled_wav(&request.source_path, &destination)
        .and_then(|()| register_fresh_duplicate(&request, &destination));
    if result.is_err() {
        let _ = fs::remove_file(&destination);
    }
    result.map(|()| ContextSampleDoubleResult { destination })
}

pub fn execute_whole_file_harvest_extraction(
    request: WholeFileHarvestExtractionRequest,
) -> WholeFileHarvestExtractionResult {
    let mut reserved_outputs = HashSet::new();
    let mut copied = Vec::with_capacity(request.copies.len());
    let mut failed = Vec::new();
    for copy in request.copies {
        let WholeFileHarvestExtractionPlan {
            source_path,
            target_folder,
            operation,
        } = copy;
        let result = next_available_reserved_whole_file_harvest_copy_path(
            &source_path,
            &target_folder,
            &reserved_outputs,
        )
        .and_then(|output_path| {
            reserved_outputs.insert(output_path.clone());
            harvest_file_ops::ensure_dir(&target_folder, "Could not create harvest destination")
                .and_then(|_| {
                    harvest_file_ops::copy_file(
                        &source_path,
                        &output_path,
                        "Could not copy selected sample",
                    )
                })
                .map(|()| WholeFileHarvestExtractionCopy {
                    source_path: source_path.clone(),
                    output_path,
                    operation: operation.clone(),
                })
        });
        match result {
            Ok(copy) => copied.push(copy),
            Err(error) => failed.push(WholeFileHarvestExtractionFailure { source_path, error }),
        }
    }
    WholeFileHarvestExtractionResult { copied, failed }
}

fn duplicate_same_destination(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap_or_else(|| Path::new(""));
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("sample"));
    let extension = source
        .extension()
        .map(|extension| extension.to_string_lossy().to_string());
    for count in 1.. {
        let file_name = match &extension {
            Some(extension) => format!("{stem}_copy{count:03}.{extension}"),
            None => format!("{stem}_copy{count:03}"),
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("unbounded copy suffix search should find a destination")
}

fn duplicate_sample_file_with_metadata(
    source: &Path,
    destination: &Path,
    source_root: &Path,
    source_database_root: &Path,
) -> Result<(), String> {
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|err| format!("Duplicate failed: {err}"))?;
    if let Err(error) = crate::sample_sources::persist_copied_file_metadata(
        source_root,
        source_database_root,
        source,
        destination,
    ) {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn next_doubled_destination(source: &Path, target_folder: &Path) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| String::from("Source sample has no file name"))?;
    for index in 0..10_000 {
        let suffix = if index == 0 {
            String::from("_doubled")
        } else {
            format!("_doubled_{index:03}")
        };
        let candidate = target_folder.join(format!("{stem}{suffix}.wav"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(String::from("Unable to find a duplicate file name"))
}

fn write_doubled_wav(source: &Path, destination: &Path) -> Result<(), String> {
    let first_reader = open_wav_reader(source)?;
    let spec = first_reader.spec();
    if first_reader.duration() == 0 {
        return Err(String::from("Source WAV contains no audio"));
    }
    let file = fs::File::create(destination)
        .map_err(|err| format!("Failed to create doubled sample: {err}"))?;
    let writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
    let mut writer = hound::WavWriter::new(writer, spec)
        .map_err(|err| format!("Failed to write doubled sample: {err}"))?;
    write_reader_samples(first_reader, spec, &mut writer)?;
    let second_reader = open_wav_reader(source)?;
    write_reader_samples(second_reader, spec, &mut writer)?;
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize doubled sample: {err}"))
}

fn open_wav_reader(source: &Path) -> Result<hound::WavReader<impl std::io::Read>, String> {
    let file = crate::wav_sanitize::open_sanitized_wav(source)
        .map_err(|err| format!("Invalid WAV: {err}"))?;
    Ok(
        hound::WavReader::new(std::io::BufReader::with_capacity(1024 * 1024, file))
            .map_err(|err| format!("Invalid WAV: {err}"))?,
    )
}

fn write_reader_samples<W: std::io::Write + std::io::Seek, R: std::io::Read>(
    mut reader: hound::WavReader<R>,
    spec: hound::WavSpec,
    writer: &mut hound::WavWriter<W>,
) -> Result<(), String> {
    match spec.sample_format {
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                writer
                    .write_sample(sample.map_err(|err| format!("Invalid WAV sample data: {err}"))?)
                    .map_err(|err| format!("Failed to write doubled sample: {err}"))?;
            }
        }
        hound::SampleFormat::Int if spec.bits_per_sample <= 16 => {
            for sample in reader.samples::<i16>() {
                writer
                    .write_sample(sample.map_err(|err| format!("Invalid WAV sample data: {err}"))?)
                    .map_err(|err| format!("Failed to write doubled sample: {err}"))?;
            }
        }
        hound::SampleFormat::Int => {
            for sample in reader.samples::<i32>() {
                writer
                    .write_sample(sample.map_err(|err| format!("Invalid WAV sample data: {err}"))?)
                    .map_err(|err| format!("Failed to write doubled sample: {err}"))?;
            }
        }
    }
    Ok(())
}

fn register_fresh_duplicate(
    request: &DuplicateDoubleRequest,
    destination: &Path,
) -> Result<(), String> {
    let relative_path = destination
        .strip_prefix(&request.target_source_root)
        .map_err(|_| String::from("Duplicate metadata update failed: target file mismatch"))?;
    let metadata = fs::metadata(destination)
        .map_err(|err| format!("Duplicate metadata update failed: {err}"))?;
    let modified_ns = metadata_modified_ns(&metadata)?;
    let db = SourceDatabase::open_for_user_metadata_write_with_database_root(
        &request.target_source_root,
        &request.target_database_root,
    )
    .map_err(|err| format!("Duplicate metadata update failed: {err}"))?;
    db.upsert_file(relative_path, metadata.len(), modified_ns)
        .map_err(|err| format!("Duplicate metadata update failed: {err}"))
}

fn metadata_modified_ns(metadata: &fs::Metadata) -> Result<i64, String> {
    Ok(system_time_to_unix_nanos(metadata.modified().map_err(
        |err| format!("Duplicate metadata update failed: {err}"),
    )?))
}

fn next_available_reserved_whole_file_harvest_copy_path(
    source_path: &Path,
    target_folder: &Path,
    reserved_outputs: &HashSet<PathBuf>,
) -> Result<PathBuf, String> {
    let stem = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| String::from("Source sample has no file name"))?;
    for index in 0..10_000 {
        let suffix = if index == 0 {
            String::from("_copy")
        } else {
            format!("_copy_{index}")
        };
        let candidate = target_folder.join(format!("{stem}{suffix}.wav"));
        if !candidate.exists() && !reserved_outputs.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(String::from(
        "Could not find an available harvest copy file name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample_sources::{Rating, SourceDatabase};

    fn write_i16_wav(path: &Path, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
        for sample in samples {
            writer.write_sample(*sample).expect("write sample");
        }
        writer.finalize().expect("finalize wav");
    }

    #[test]
    fn duplicate_same_numbers_collisions_and_preserves_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let source = root.join("kick.wav");
        let first_collision = root.join("kick_copy001.wav");
        fs::write(&source, [1_u8, 2, 3, 4]).expect("write source");
        fs::write(&first_collision, [9_u8]).expect("write collision");
        let db = SourceDatabase::open_for_source_write(root).expect("open db");
        db.upsert_file(Path::new("kick.wav"), 4, 1)
            .expect("upsert source");
        db.set_tag(Path::new("kick.wav"), Rating::new(2))
            .expect("set rating");

        let result = execute_duplicate_context_sample_same(DuplicateSameRequest {
            source_path: source.clone(),
            source_root: root.to_path_buf(),
            source_database_root: root.to_path_buf(),
        })
        .expect("duplicate same");

        assert_eq!(result.destination, root.join("kick_copy002.wav"));
        assert_eq!(
            fs::read(&result.destination).expect("read duplicate"),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            db.tag_for_path(Path::new("kick_copy002.wav"))
                .expect("read duplicate rating"),
            Some(Rating::new(2))
        );
        assert!(source.exists(), "original should stay in place");
    }

    #[test]
    fn duplicate_same_cleans_up_file_when_metadata_write_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("kick.wav");
        let destination = temp.path().join("kick_copy001.wav");
        fs::write(&source, [1_u8, 2, 3, 4]).expect("write source");

        let error = execute_duplicate_context_sample_same(DuplicateSameRequest {
            source_path: source,
            source_root: temp.path().join("not-the-source-root"),
            source_database_root: temp.path().to_path_buf(),
        })
        .expect_err("metadata mismatch should fail");

        assert!(error.contains("source file mismatch"));
        assert!(!destination.exists(), "failed duplicate should be removed");
    }

    #[test]
    fn doubled_wav_repeats_audio_and_preserves_spec() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("loop.wav");
        let destination = temp.path().join("loop_doubled.wav");
        write_i16_wav(&source, &[1, -2, 3, -4, 5, -6]);

        write_doubled_wav(&source, &destination).expect("double wav");

        let mut reader = hound::WavReader::open(&destination).expect("open doubled wav");
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(reader.duration(), 6);
        let samples = reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .expect("read samples");
        assert_eq!(samples, vec![1, -2, 3, -4, 5, -6, 1, -2, 3, -4, 5, -6]);
    }

    #[test]
    fn doubled_destination_numbers_collisions() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("kick.wav");
        fs::write(temp.path().join("kick_doubled.wav"), []).expect("collision");

        let destination = next_doubled_destination(&source, temp.path()).expect("destination");

        assert_eq!(destination, temp.path().join("kick_doubled_001.wav"));
    }

    #[test]
    fn execute_double_registers_fresh_duplicate_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("loop.wav");
        write_i16_wav(&source, &[1, -2, 3, -4]);

        let result = execute_duplicate_context_sample_double(DuplicateDoubleRequest {
            source_path: source,
            target_folder: temp.path().to_path_buf(),
            target_source_root: temp.path().to_path_buf(),
            target_database_root: temp.path().to_path_buf(),
        })
        .expect("duplicate double");

        assert_eq!(result.destination, temp.path().join("loop_doubled.wav"));
        let db = SourceDatabase::open_for_source_write(temp.path()).expect("open db");
        assert!(
            db.entry_for_path(Path::new("loop_doubled.wav"))
                .expect("read doubled entry")
                .is_some()
        );
    }

    #[test]
    fn execute_double_cleans_up_file_when_metadata_write_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("loop.wav");
        let destination = temp.path().join("loop_doubled.wav");
        write_i16_wav(&source, &[1, -2, 3, -4]);

        let error = execute_duplicate_context_sample_double(DuplicateDoubleRequest {
            source_path: source,
            target_folder: temp.path().to_path_buf(),
            target_source_root: temp.path().join("not-the-target-root"),
            target_database_root: temp.path().to_path_buf(),
        })
        .expect_err("metadata mismatch should fail");

        assert!(error.contains("target file mismatch"));
        assert!(
            !destination.exists(),
            "failed doubled file should be removed"
        );
    }

    #[test]
    fn whole_file_harvest_extraction_reserves_colliding_outputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source_a = temp.path().join("kick.wav");
        let source_b = temp.path().join("nested").join("kick.wav");
        let target = temp.path().join("harvest");
        fs::create_dir_all(source_b.parent().expect("nested parent")).expect("create nested");
        fs::create_dir_all(&target).expect("create target");
        fs::write(&source_a, [1_u8]).expect("write source a");
        fs::write(&source_b, [2_u8]).expect("write source b");
        fs::write(target.join("kick_copy.wav"), [9_u8]).expect("write collision");

        let result = execute_whole_file_harvest_extraction(WholeFileHarvestExtractionRequest {
            copies: vec![
                WholeFileHarvestExtractionPlan {
                    source_path: source_a,
                    target_folder: target.clone(),
                    operation: HarvestDerivationOperation::Copy,
                },
                WholeFileHarvestExtractionPlan {
                    source_path: source_b,
                    target_folder: target.clone(),
                    operation: HarvestDerivationOperation::Copy,
                },
            ],
        });

        assert!(result.failed.is_empty());
        assert_eq!(result.copied[0].output_path, target.join("kick_copy_1.wav"));
        assert_eq!(result.copied[1].output_path, target.join("kick_copy_2.wav"));
    }
}
