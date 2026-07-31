use std::{fs, io, path::Path};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InjectedFailure {
    None,
    Write,
    Finalize,
    Replace,
    ReplaceAndRestore,
}

struct TestAtomicEditIo {
    failure: InjectedFailure,
}

impl TestAtomicEditIo {
    fn new(failure: InjectedFailure) -> Self {
        Self { failure }
    }
}

impl AtomicEditIo for TestAtomicEditIo {
    fn before_sample_write(&self, sample_index: usize) -> Result<(), String> {
        if self.failure == InjectedFailure::Write && sample_index == 1 {
            return Err(String::from("injected late write failure"));
        }
        Ok(())
    }

    fn before_finalize(&self) -> Result<(), String> {
        if self.failure == InjectedFailure::Finalize {
            return Err(String::from("injected finalize failure"));
        }
        Ok(())
    }

    fn replace(&self, staged: &Path, target: &Path, phase: ReplacePhase) -> io::Result<()> {
        match (self.failure, phase) {
            (InjectedFailure::Replace, ReplacePhase::Commit) => {
                Err(io::Error::other("injected replace failure"))
            }
            (InjectedFailure::ReplaceAndRestore, ReplacePhase::Commit) => {
                fs::write(target, b"damaged replacement")?;
                Err(io::Error::other("injected late replace failure"))
            }
            (InjectedFailure::ReplaceAndRestore, ReplacePhase::Restore) => {
                Err(io::Error::other("injected restore failure"))
            }
            _ => replace_file(staged, target),
        }
    }
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("waveform atomic-write fixture");
    let target = directory.path().join("sample.wav");
    let recovery = directory.path().join("before.wav");
    fs::write(&target, b"original audio bytes").expect("write original fixture");
    fs::copy(&target, &recovery).expect("write recovery fixture");
    (directory, target, recovery)
}

fn run_with_failure(
    failure: InjectedFailure,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    AtomicWriteFailure,
) {
    let (directory, target, recovery) = fixture();
    let after_snapshot = directory.path().join("after.wav");
    let error = write_wav_atomically_with(
        &target,
        &recovery,
        &after_snapshot,
        1,
        48_000,
        &[0.0, 0.25, -0.25, 0.5],
        &TestAtomicEditIo::new(failure),
    )
    .expect_err("injected failure should fail the edit");
    (directory, target, recovery, error)
}

#[test]
fn write_failure_leaves_original_bytes_unchanged() {
    let (_directory, target, recovery, error) = run_with_failure(InjectedFailure::Write);

    assert_eq!(fs::read(&target).unwrap(), fs::read(&recovery).unwrap());
    assert!(!error.recovery_copy_required());
    assert!(error.to_string().contains("late write failure"));
}

#[test]
fn finalize_failure_leaves_original_bytes_unchanged() {
    let (_directory, target, recovery, error) = run_with_failure(InjectedFailure::Finalize);

    assert_eq!(fs::read(&target).unwrap(), fs::read(&recovery).unwrap());
    assert!(!error.recovery_copy_required());
    assert!(error.to_string().contains("finalize failure"));
}

#[test]
fn redo_snapshot_failure_leaves_original_bytes_unchanged() {
    let (directory, target, recovery) = fixture();
    let after_snapshot = directory.path().join("missing").join("after.wav");

    let error = write_wav_atomically_with(
        &target,
        &recovery,
        &after_snapshot,
        1,
        48_000,
        &[0.0, 0.25, -0.25, 0.5],
        &TestAtomicEditIo::new(InjectedFailure::None),
    )
    .expect_err("snapshot failure should fail the edit");

    assert_eq!(fs::read(&target).unwrap(), fs::read(&recovery).unwrap());
    assert!(!error.recovery_copy_required());
    assert!(error.to_string().contains("snapshot edited audio file"));
}

#[test]
fn replace_failure_restores_original_bytes() {
    let (_directory, target, recovery, error) = run_with_failure(InjectedFailure::Replace);

    assert_eq!(fs::read(&target).unwrap(), fs::read(&recovery).unwrap());
    assert_eq!(
        fs::metadata(&target).unwrap().permissions(),
        fs::metadata(&recovery).unwrap().permissions()
    );
    assert!(!error.recovery_copy_required());
    assert!(error.to_string().contains("original audio was restored"));
}

#[test]
fn rollback_failure_reports_the_exact_recovery_path() {
    let (_directory, target, recovery, error) =
        run_with_failure(InjectedFailure::ReplaceAndRestore);

    assert_eq!(fs::read(&target).unwrap(), b"damaged replacement");
    assert_eq!(fs::read(&recovery).unwrap(), b"original audio bytes");
    assert!(error.recovery_copy_required());
    assert!(error.to_string().contains(&recovery.display().to_string()));
    assert!(error.to_string().contains("injected restore failure"));
}

#[test]
fn successful_commit_publishes_a_complete_synced_wav() {
    let (directory, target, recovery) = fixture();
    let after_snapshot = directory.path().join("after.wav");
    let original_permissions = fs::metadata(&target).unwrap().permissions();

    write_wav_atomically_with(
        &target,
        &recovery,
        &after_snapshot,
        1,
        48_000,
        &[0.0, 0.25, -0.25, 0.5],
        &TestAtomicEditIo::new(InjectedFailure::None),
    )
    .expect("atomic waveform commit");

    assert_ne!(fs::read(&target).unwrap(), fs::read(&recovery).unwrap());
    assert_eq!(
        fs::read(&target).unwrap(),
        fs::read(&after_snapshot).unwrap()
    );
    assert_eq!(
        fs::metadata(&target).unwrap().permissions(),
        original_permissions
    );
    let mut reader = hound::WavReader::open(&target).expect("committed WAV");
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(reader.spec().sample_rate, 48_000);
    assert_eq!(reader.samples::<f32>().count(), 4);
}

#[cfg(unix)]
#[test]
fn explicit_read_write_modes_survive_commit_and_rollback() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, target, recovery) = fixture();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    fs::set_permissions(&recovery, fs::Permissions::from_mode(0o440)).unwrap();
    let after = directory.path().join("after.wav");
    write_wav_atomically_with(
        &target,
        &recovery,
        &after,
        1,
        48_000,
        &[0.0, 0.25, -0.25, 0.5],
        &TestAtomicEditIo::new(InjectedFailure::None),
    )
    .unwrap();
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(&recovery).unwrap().permissions().mode() & 0o777,
        0o440
    );
}
