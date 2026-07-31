//! Bounded pre-intent capacity admission for history-backed waveform restores.
//!
//! This module intentionally owns no filesystem mutation.  It maps one narrowly defined
//! history shape, discovers destination-volume facts from a live descriptor, aggregates
//! allocation claims, and gives the journal owner the durable plan to persist.

#![allow(missing_docs)]
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::file_io::{HistoryFileAction, HistoryFileIoDirection};

/// Minimum free space retained after logical claims are admitted.
pub(crate) const PROTECTED_FREE_SPACE_FLOOR: u64 = 256 * 1024 * 1024;

/// Stable identity obtained from the destination file descriptor.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct VolumeIdentity {
    /// Unix `st_dev`; other platforms may use their native volume serial in a future slice.
    pub(crate) device: u64,
}

/// Physical allocation accounting class used by a capacity claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum CapacityAllocationClass {
    /// Destination-side staging bytes (the only charged class in this bounded slice).
    DestinationStaging,
    /// Bytes for a future operation-journal record.
    JournalRecord,
    /// Bytes for a future source-database commit.
    SourceDatabaseCommit,
    /// Bytes for a future source-database WAL/SHM pair.
    SourceWalShm,
    /// Bytes for a future global-database commit.
    GlobalDatabaseCommit,
    /// Bytes for a future global-database WAL/SHM pair.
    GlobalWalShm,
    /// Existing destination allocation retained by the operation.
    ExistingDestination,
    /// Existing recovery payload retained by the operation.
    ExistingRecoveryPayload,
}

/// One volume's durable capacity claim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DurableVolumeCapacity {
    pub(crate) identity: VolumeIdentity,
    pub(crate) allocation_unit: u64,
    pub(crate) allocation_class: CapacityAllocationClass,
    pub(crate) logical_bytes: u64,
    pub(crate) allocated_bytes: u64,
    #[serde(default)]
    pub(crate) protected_free_bytes: u64,
}

/// Additive journal payload.  The field on `OperationRecord` is optional for legacy records.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DurableCapacityPlan {
    #[serde(default)]
    pub(crate) volumes: Vec<DurableVolumeCapacity>,
}

/// The only history shape admitted by this bounded slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BoundedWaveformRestoreAdmission {
    pub(crate) direction: HistoryFileIoDirection,
    pub(crate) backup_path: PathBuf,
    pub(crate) target_path: PathBuf,
}

/// Fail-closed reasons returned before an intent can be written.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum RejectedBeforeIntent {
    #[error("history action shape is outside the bounded capacity gate")]
    InvalidShape,
    #[error("history restore backup is missing: {0}")]
    MissingBackup(PathBuf),
    #[error("history restore target is missing: {0}")]
    MissingTarget(PathBuf),
    #[error("history restore path is not a regular file: {0}")]
    NotRegularFile(PathBuf),
    #[error("capacity facts unavailable: {0}")]
    Discovery(String),
    #[error("capacity arithmetic overflow")]
    Overflow,
    #[error("capacity plan is invalid")]
    InvalidPlan,
    #[error("capacity admission is blocked by unresolved journal recovery")]
    RecoveryBlocked,
    #[error("insufficient free space on volume {0:?}: need {1} bytes, have {2} bytes")]
    InsufficientSpace(VolumeIdentity, u64, u64),
}

pub(crate) type CapacityGateError = RejectedBeforeIntent;

/// Pure shape mapper.  No filesystem calls occur here.
pub(crate) fn map_waveform_restore_shape(
    direction: HistoryFileIoDirection,
    actions: &[HistoryFileAction],
) -> Result<BoundedWaveformRestoreAdmission, CapacityGateError> {
    if actions.len() != 1 {
        return Err(CapacityGateError::InvalidShape);
    }
    let HistoryFileAction::WaveformRestore {
        backup_path,
        applied,
    } = &actions[0]
    else {
        return Err(CapacityGateError::InvalidShape);
    };
    if applied.extracted.is_some()
        || !backup_path.is_absolute()
        || !applied.absolute_path.is_absolute()
        || backup_path.as_os_str().is_empty()
        || applied.absolute_path.as_os_str().is_empty()
    {
        return Err(CapacityGateError::InvalidShape);
    }
    if backup_path != &applied.backup.before && backup_path != &applied.backup.after {
        return Err(CapacityGateError::InvalidShape);
    }
    Ok(BoundedWaveformRestoreAdmission {
        direction,
        backup_path: backup_path.clone(),
        target_path: applied.absolute_path.clone(),
    })
}

/// Facts obtained while holding an open destination descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VolumeFacts {
    pub(crate) identity: VolumeIdentity,
    pub(crate) free_bytes: u64,
    pub(crate) allocation_unit: u64,
}

/// One logical write requirement before aggregation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapacityRequirement {
    pub(crate) facts: VolumeFacts,
    pub(crate) logical_bytes: u64,
}

/// Round a logical byte count to physical allocation units, rejecting zero units and overflow.
pub(crate) fn round_up_allocation(
    bytes: u64,
    allocation_unit: u64,
) -> Result<u64, CapacityGateError> {
    if allocation_unit == 0 {
        return Err(CapacityGateError::InvalidPlan);
    }
    if bytes == 0 {
        return Ok(0);
    }
    let remainder = bytes % allocation_unit;
    if remainder == 0 {
        return Ok(bytes);
    }
    bytes
        .checked_add(allocation_unit - remainder)
        .ok_or(CapacityGateError::Overflow)
}

/// Aggregate logical requirements by descriptor-derived volume identity and enforce free space.
pub(crate) fn aggregate_capacity_plan(
    requirements: &[CapacityRequirement],
    existing_claims: &BTreeMap<VolumeIdentity, u64>,
) -> Result<DurableCapacityPlan, CapacityGateError> {
    let mut grouped: BTreeMap<VolumeIdentity, DurableVolumeCapacity> = BTreeMap::new();
    for requirement in requirements {
        let allocated_bytes =
            round_up_allocation(requirement.logical_bytes, requirement.facts.allocation_unit)?;
        let entry = grouped
            .entry(requirement.facts.identity.clone())
            .or_insert_with(|| DurableVolumeCapacity {
                identity: requirement.facts.identity.clone(),
                allocation_unit: requirement.facts.allocation_unit,
                allocation_class: CapacityAllocationClass::DestinationStaging,
                logical_bytes: 0,
                allocated_bytes: 0,
                protected_free_bytes: PROTECTED_FREE_SPACE_FLOOR,
            });
        if entry.allocation_unit != requirement.facts.allocation_unit {
            return Err(CapacityGateError::InvalidPlan);
        }
        entry.logical_bytes = entry
            .logical_bytes
            .checked_add(requirement.logical_bytes)
            .ok_or(CapacityGateError::Overflow)?;
        entry.allocated_bytes = entry
            .allocated_bytes
            .checked_add(allocated_bytes)
            .ok_or(CapacityGateError::Overflow)?;
    }
    for claim in grouped.values() {
        let already_claimed = existing_claims.get(&claim.identity).copied().unwrap_or(0);
        let reserved = already_claimed
            .checked_add(claim.allocated_bytes)
            .ok_or(CapacityGateError::Overflow)?;
        let free_bytes = requirements
            .iter()
            .filter(|requirement| requirement.facts.identity == claim.identity)
            .map(|requirement| requirement.facts.free_bytes)
            .min()
            .ok_or(CapacityGateError::InvalidPlan)?;
        if reserved
            .checked_add(PROTECTED_FREE_SPACE_FLOOR)
            .ok_or(CapacityGateError::Overflow)?
            > free_bytes
        {
            return Err(CapacityGateError::InsufficientSpace(
                claim.identity.clone(),
                claim.allocated_bytes,
                free_bytes
                    .saturating_sub(already_claimed)
                    .saturating_sub(PROTECTED_FREE_SPACE_FLOOR),
            ));
        }
    }
    Ok(DurableCapacityPlan {
        volumes: grouped.into_values().collect(),
    })
}

/// Discover descriptor-derived capacity facts for one target and backup pair.
pub(crate) fn discover_requirement(
    admission: &BoundedWaveformRestoreAdmission,
) -> Result<CapacityRequirement, CapacityGateError> {
    let backup = open_regular_file(&admission.backup_path, CapacityGateError::MissingBackup)?;
    let logical_bytes = backup
        .metadata()
        .map_err(|error| CapacityGateError::Discovery(error.to_string()))?
        .len();
    let target = open_regular_file(&admission.target_path, CapacityGateError::MissingTarget)?;
    let facts = descriptor_volume_facts(&target).map_err(|error| {
        CapacityGateError::Discovery(format!("{}: {error}", admission.target_path.display()))
    })?;
    Ok(CapacityRequirement {
        facts,
        logical_bytes,
    })
}

fn open_regular_file(
    path: &Path,
    missing: fn(PathBuf) -> CapacityGateError,
) -> Result<File, CapacityGateError> {
    let file = File::open(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            missing(path.to_path_buf())
        } else {
            CapacityGateError::Discovery(error.to_string())
        }
    })?;
    let metadata = file
        .metadata()
        .map_err(|error| CapacityGateError::Discovery(error.to_string()))?;
    if !metadata.is_file() {
        return Err(CapacityGateError::NotRegularFile(path.to_path_buf()));
    }
    Ok(file)
}

#[cfg(unix)]
fn descriptor_volume_facts(file: &File) -> Result<VolumeFacts, io::Error> {
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stat = unsafe { stat.assume_init() };
    let mut fs_stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    if unsafe { libc::fstatvfs(fd, fs_stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let fs_stat = unsafe { fs_stat.assume_init() };
    let (free, unit) = statvfs_capacity_values(
        u64::try_from(fs_stat.f_bavail)
            .map_err(|_| io::Error::other("invalid free-space block count"))?,
        u64::try_from(fs_stat.f_frsize)
            .map_err(|_| io::Error::other("invalid filesystem fragment size"))?,
        u64::try_from(fs_stat.f_bsize)
            .map_err(|_| io::Error::other("invalid filesystem block size"))?,
    )?;
    Ok(VolumeFacts {
        identity: VolumeIdentity {
            device: stat.st_dev as u64,
        },
        free_bytes: free,
        allocation_unit: unit,
    })
}

#[cfg(unix)]
fn statvfs_capacity_values(
    available_blocks: u64,
    fragment_size: u64,
    block_size: u64,
) -> Result<(u64, u64), io::Error> {
    if fragment_size == 0 || block_size == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "filesystem allocation unit is zero",
        ));
    }
    let free_bytes = available_blocks
        .checked_mul(fragment_size)
        .ok_or_else(|| io::Error::other("free-space arithmetic overflow"))?;
    Ok((free_bytes, fragment_size))
}

#[cfg(windows)]
fn descriptor_volume_facts(_file: &File) -> Result<VolumeFacts, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-derived volume facts are not implemented on Windows",
    ))
}

#[cfg(not(any(unix, windows)))]
fn descriptor_volume_facts(_file: &File) -> Result<VolumeFacts, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-derived volume facts are not implemented on this platform",
    ))
}

/// Build the one-action plan used by the journal owner.
pub(crate) fn plan_waveform_restore(
    direction: HistoryFileIoDirection,
    actions: &[HistoryFileAction],
    existing_claims: &BTreeMap<VolumeIdentity, u64>,
) -> Result<(BoundedWaveformRestoreAdmission, DurableCapacityPlan), CapacityGateError> {
    let admission = map_waveform_restore_shape(direction, actions)?;
    let requirement = discover_requirement(&admission)?;
    let plan = aggregate_capacity_plan(&[requirement], existing_claims)?;
    Ok((admission, plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests;

    fn facts(device: u64, free_bytes: u64, allocation_unit: u64) -> VolumeFacts {
        VolumeFacts {
            identity: VolumeIdentity { device },
            free_bytes,
            allocation_unit,
        }
    }

    #[test]
    fn shape_mapper_accepts_one_restore_for_both_directions() {
        let backup = PathBuf::from("/tmp/before.wav");
        let target = PathBuf::from("/tmp/target.wav");
        for direction in [HistoryFileIoDirection::Undo, HistoryFileIoDirection::Redo] {
            let mapped = map_waveform_restore_shape(
                direction,
                &[waveform_restore_action_for_capacity_tests(
                    backup.clone(),
                    target.clone(),
                    false,
                )],
            )
            .unwrap();
            assert_eq!(mapped.direction, direction);
            assert_eq!(mapped.backup_path, backup);
            assert_eq!(mapped.target_path, target);
        }
    }

    #[test]
    fn shape_mapper_rejects_compound_folder_extracted_and_mismatched_shapes() {
        let backup = PathBuf::from("/tmp/before.wav");
        let target = PathBuf::from("/tmp/target.wav");
        let restore =
            waveform_restore_action_for_capacity_tests(backup.clone(), target.clone(), false);
        assert_eq!(
            map_waveform_restore_shape(HistoryFileIoDirection::Undo, &[]),
            Err(CapacityGateError::InvalidShape)
        );
        assert!(matches!(
            map_waveform_restore_shape(
                HistoryFileIoDirection::Undo,
                &[restore.clone(), restore.clone()],
            ),
            Err(CapacityGateError::InvalidShape)
        ));
        assert!(matches!(
            map_waveform_restore_shape(
                HistoryFileIoDirection::Undo,
                &[HistoryFileAction::FolderMove {
                    source_root: PathBuf::from("/tmp/source"),
                    source_database_root: PathBuf::from("/tmp/db"),
                    moves: Vec::new(),
                }],
            ),
            Err(CapacityGateError::InvalidShape)
        ));
        assert!(matches!(
            map_waveform_restore_shape(
                HistoryFileIoDirection::Undo,
                &[waveform_restore_action_for_capacity_tests(
                    backup.clone(),
                    target.clone(),
                    true,
                )],
            ),
            Err(CapacityGateError::InvalidShape)
        ));
        let mut mismatch =
            waveform_restore_action_for_capacity_tests(backup.clone(), target.clone(), false);
        if let HistoryFileAction::WaveformRestore { backup_path, .. } = &mut mismatch {
            *backup_path = PathBuf::from("/tmp/other.wav");
        }
        assert!(matches!(
            map_waveform_restore_shape(HistoryFileIoDirection::Undo, &[mismatch]),
            Err(CapacityGateError::InvalidShape)
        ));
        let relative = waveform_restore_action_for_capacity_tests(
            PathBuf::from("before.wav"),
            PathBuf::from("/tmp/target.wav"),
            false,
        );
        assert!(matches!(
            map_waveform_restore_shape(HistoryFileIoDirection::Undo, &[relative]),
            Err(CapacityGateError::InvalidShape)
        ));
    }

    #[test]
    fn rounds_floor_and_exact_boundary() {
        assert_eq!(round_up_allocation(0, 4096).unwrap(), 0);
        assert_eq!(round_up_allocation(4096, 4096).unwrap(), 4096);
        assert_eq!(round_up_allocation(4097, 4096).unwrap(), 8192);
    }

    #[test]
    fn aggregates_same_and_two_volumes() {
        let requirements = vec![
            CapacityRequirement {
                facts: facts(1, PROTECTED_FREE_SPACE_FLOOR + 16 * 1024, 4096),
                logical_bytes: 4097,
            },
            CapacityRequirement {
                facts: facts(1, PROTECTED_FREE_SPACE_FLOOR + 16 * 1024, 4096),
                logical_bytes: 4096,
            },
            CapacityRequirement {
                facts: facts(2, PROTECTED_FREE_SPACE_FLOOR + 8 * 1024, 4096),
                logical_bytes: 4097,
            },
        ];
        let plan = aggregate_capacity_plan(&requirements, &BTreeMap::new()).unwrap();
        assert_eq!(plan.volumes.len(), 2);
        assert_eq!(plan.volumes[0].logical_bytes, 8193);
        assert_eq!(plan.volumes[0].allocated_bytes, 12288);
        assert_eq!(plan.volumes[1].allocated_bytes, 8192);
    }

    #[test]
    fn claims_consume_free_space_and_overflow_fails_closed() {
        let mut claims = BTreeMap::new();
        claims.insert(VolumeIdentity { device: 1 }, 4096);
        assert!(matches!(
            aggregate_capacity_plan(
                &[CapacityRequirement {
                    facts: facts(1, PROTECTED_FREE_SPACE_FLOOR + 8192, 4096),
                    logical_bytes: 4097,
                }],
                &claims,
            ),
            Err(CapacityGateError::InsufficientSpace(..))
        ));
        assert!(matches!(
            round_up_allocation(u64::MAX, 4096),
            Err(CapacityGateError::Overflow)
        ));
        assert!(
            aggregate_capacity_plan(
                &[CapacityRequirement {
                    facts: facts(3, PROTECTED_FREE_SPACE_FLOOR + 4096, 4096),
                    logical_bytes: 4096,
                }],
                &BTreeMap::new(),
            )
            .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn statvfs_free_bytes_use_fragment_size_not_block_size() {
        assert_eq!(
            statvfs_capacity_values(2, 4096, 16384).unwrap(),
            (8192, 4096)
        );
        assert!(statvfs_capacity_values(2, 0, 16384).is_err());
        assert!(statvfs_capacity_values(2, 4096, 0).is_err());
    }
}
