//! Capability-bound qualification for an absent-final, no-replace claim.
//!
//! The legacy borrowed/owned adoption qualification adapters are test-only. Production publication
//! uses the owner-facing guard function, which acquires and consumes the capability-bound result
//! without exposing a generic publication constructor or a recovery/adoption coordinator seam.

#![allow(dead_code)]

use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::operation_journal::{
    AbsentFinalAdoptionGuardRequest, OperationId, PreparedFileEvidence, PreparedObjectIdentity,
};
use super::publication::{PublicationSynchronization, PublicationVisibility};

mod sealed {
    pub trait Sealed {}
}

/// A leaf in the target-parent namespace must be one clean, normal relative component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AbsentFinalNoReplaceRequestError {
    TargetLeafNotSingleCleanNormal,
    StagingLeafNotSingleCleanNormal,
}

/// A capability-bound absent-final request.
pub(super) struct AbsentFinalNoReplaceRequest<'a> {
    target_parent: &'a File,
    staging: &'a File,
    target_leaf: &'a Path,
    staging_leaf: &'a Path,
    expected_target_parent: &'a PreparedObjectIdentity,
    expected_staging: &'a PreparedObjectIdentity,
    expected_content: &'a PreparedFileEvidence,
}

/// A read-only request for descriptor-relative adoption qualification.
///
/// The owner guard and the legacy test-only adapter borrow the capability and carry only durable
/// identity/content expectations. The request contains no pathname outside the clean final leaf
/// and grants no namespace mutation authority.
pub(super) struct AbsentFinalAdoptionRequest<'a> {
    target_parent: &'a File,
    final_leaf: &'a Path,
    expected_target_parent: &'a PreparedObjectIdentity,
    expected_final_stable_id: &'a str,
    expected_final_len: u64,
    expected_final_content: &'a [u8; 32],
}

/// Test-only owned request for the legacy coordinator recovery/adoption seam.
///
/// The journal constructs this from durable preparation, recovery observation, and transaction-
/// owned proof. The adapter reacquires the target-parent capability and revalidates the final;
/// this request carries no borrowed descriptor.
#[cfg(test)]
pub(super) struct AbsentFinalAdoptionQualificationRequest {
    pub(super) operation_id: OperationId,
    pub(super) target_parent_path: PathBuf,
    pub(super) final_leaf: PathBuf,
    pub(super) expected_target_parent_stable_id: String,
    pub(super) expected_final_stable_id: String,
    pub(super) expected_final_len: u64,
    pub(super) expected_final_content: [u8; 32],
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AbsentFinalAdoptionQualificationRequestError {
    TargetParentPathNotAbsolute,
    FinalLeafNotSingleCleanNormal,
    TargetParentIdentityEmpty,
    FinalIdentityEmpty,
}

#[cfg(test)]
impl AbsentFinalAdoptionQualificationRequest {
    pub(super) fn try_new(
        operation_id: OperationId,
        target_parent_path: PathBuf,
        final_leaf: PathBuf,
        expected_target_parent_stable_id: String,
        expected_final_stable_id: String,
        expected_final_len: u64,
        expected_final_content: [u8; 32],
    ) -> Result<Self, AbsentFinalAdoptionQualificationRequestError> {
        if !target_parent_path.is_absolute() {
            return Err(AbsentFinalAdoptionQualificationRequestError::TargetParentPathNotAbsolute);
        }
        if !is_single_clean_normal_leaf(&final_leaf) {
            return Err(
                AbsentFinalAdoptionQualificationRequestError::FinalLeafNotSingleCleanNormal,
            );
        }
        if expected_target_parent_stable_id.is_empty() {
            return Err(AbsentFinalAdoptionQualificationRequestError::TargetParentIdentityEmpty);
        }
        if expected_final_stable_id.is_empty() {
            return Err(AbsentFinalAdoptionQualificationRequestError::FinalIdentityEmpty);
        }
        Ok(Self {
            operation_id,
            target_parent_path,
            final_leaf,
            expected_target_parent_stable_id,
            expected_final_stable_id,
            expected_final_len,
            expected_final_content,
        })
    }
}

impl<'a> AbsentFinalAdoptionRequest<'a> {
    pub(super) fn try_new(
        target_parent: &'a File,
        final_leaf: &'a Path,
        expected_target_parent: &'a PreparedObjectIdentity,
        expected_final_stable_id: &'a str,
        expected_final_len: u64,
        expected_final_content: &'a [u8; 32],
    ) -> Result<Self, AbsentFinalNoReplaceRequestError> {
        if !is_single_clean_normal_leaf(final_leaf) {
            return Err(AbsentFinalNoReplaceRequestError::TargetLeafNotSingleCleanNormal);
        }
        if expected_final_stable_id.is_empty() {
            return Err(AbsentFinalNoReplaceRequestError::TargetLeafNotSingleCleanNormal);
        }
        Ok(Self {
            target_parent,
            final_leaf,
            expected_target_parent,
            expected_final_stable_id,
            expected_final_len,
            expected_final_content,
        })
    }
}

impl AbsentFinalAdoptionGuardRequest {
    pub(super) fn try_new(
        operation_id: OperationId,
        target_parent_path: PathBuf,
        final_leaf: PathBuf,
        expected_target_parent_stable_id: String,
        expected_final_stable_id: String,
        expected_final_len: u64,
        expected_final_content: [u8; 32],
    ) -> Result<Self, AbsentFinalNoReplaceRequestError> {
        if !is_single_clean_normal_leaf(&final_leaf) {
            return Err(AbsentFinalNoReplaceRequestError::TargetLeafNotSingleCleanNormal);
        }
        if expected_target_parent_stable_id.is_empty() || expected_final_stable_id.is_empty() {
            return Err(AbsentFinalNoReplaceRequestError::TargetLeafNotSingleCleanNormal);
        }
        Ok(Self {
            operation_id,
            target_parent_path,
            final_leaf,
            expected_target_parent_stable_id,
            expected_final_stable_id,
            expected_final_len,
            expected_final_content,
        })
    }

    pub(super) fn operation_id(&self) -> OperationId {
        self.operation_id
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum AbsentFinalAdoptionOutcome {
    #[cfg(test)]
    Qualified(QualifiedAbsentFinalAdoption),
    OperationIdDrift,
    ParentIdentityDrift,
    FinalMissing,
    FinalIdentityDrift,
    FinalContentDrift,
    UnsupportedPlatform,
    VerificationFailed,
}

/// Test-only transient evidence from one descriptor-relative adoption qualification.
/// It intentionally contains no path or open handle.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QualifiedAbsentFinalAdoption {
    pub(super) target_parent: PreparedObjectIdentity,
    pub(super) final_object: PreparedObjectIdentity,
    pub(super) final_content: PreparedFileEvidence,
}

/// Test-only operation-fenced result from the legacy adoption qualification seam.
/// It contains no retained handle or mutation capability.
#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(super) struct AbsentFinalAdoptionQualificationResult {
    pub(super) operation_id: OperationId,
    pub(super) outcome: AbsentFinalAdoptionOutcome,
}

#[cfg(test)]
impl AbsentFinalAdoptionQualificationResult {
    pub(super) fn operation_id(&self) -> OperationId {
        self.operation_id
    }
}

/// A retained, read-only capability for a qualified absent-final adoption.
///
/// This deliberately has no serialization, comparison, or clone boundary. The retained
/// descriptors, clean leaf, and exact content hash are the authority for a later binding check;
/// the configured root pathname is not retained or consulted.
pub(super) struct QualifiedAbsentFinalAdoptionGuard {
    operation_id: OperationId,
    target_parent: File,
    final_object: File,
    final_leaf: PathBuf,
    target_parent_identity: PreparedObjectIdentity,
    final_identity: PreparedObjectIdentity,
    final_content: [u8; 32],
}

impl QualifiedAbsentFinalAdoptionGuard {
    pub(super) fn revalidate_binding(
        &self,
        current_operation_id: OperationId,
    ) -> Result<(), AbsentFinalAdoptionOutcome> {
        #[cfg(unix)]
        {
            if self.operation_id != current_operation_id {
                return Err(AbsentFinalAdoptionOutcome::OperationIdDrift);
            }
            return revalidate_unix_absent_final_adoption_guard(self);
        }
        #[cfg(not(unix))]
        {
            let _ = (self, current_operation_id);
            Err(AbsentFinalAdoptionOutcome::UnsupportedPlatform)
        }
    }

    pub(super) fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    fn into_operation_bound_publication(
        self,
        expected_operation_id: OperationId,
    ) -> Result<OperationBoundQualifiedAbsentFinalNoReplace, AbsentFinalAdoptionOutcome> {
        // Keep the guard alive until the last possible moment.  This rechecks the retained
        // parent descriptor, retained final descriptor, and the freshly reopened final through
        // that same descriptor before any publication evidence is created.
        self.revalidate_binding(expected_operation_id)?;

        let operation_id = self.operation_id;
        Ok(OperationBoundQualifiedAbsentFinalNoReplace {
            operation_id,
            qualified: QualifiedAbsentFinalNoReplace {
                target_parent_identity: self.target_parent_identity,
                root_path_continuity: RootPathContinuity::NotClaimed,
                reopened_final: self.final_identity,
                reopened_content: PreparedFileEvidence::ContentHash(self.final_content),
                visibility: PublicationVisibility::VisibilityVerified,
                synchronization: PublicationSynchronization::SyncUnsupportedOrUnverified,
            },
        })
    }
}

/// Test-only legacy adapter; production publication uses the owner-facing guard function below.
#[cfg(test)]
pub(super) trait AbsentFinalAdoptionAdapter: sealed::Sealed {
    fn qualify(&self, request: AbsentFinalAdoptionRequest<'_>) -> AbsentFinalAdoptionOutcome;
    fn qualify_owned(
        &self,
        request: AbsentFinalAdoptionQualificationRequest,
    ) -> AbsentFinalAdoptionQualificationResult;
}

pub(super) struct ProductionAbsentFinalAdoptionAdapter;

impl sealed::Sealed for ProductionAbsentFinalAdoptionAdapter {}

#[cfg(test)]
impl AbsentFinalAdoptionAdapter for ProductionAbsentFinalAdoptionAdapter {
    fn qualify(&self, request: AbsentFinalAdoptionRequest<'_>) -> AbsentFinalAdoptionOutcome {
        #[cfg(unix)]
        {
            return qualify_unix_absent_final_adoption(request);
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            AbsentFinalAdoptionOutcome::UnsupportedPlatform
        }
    }

    fn qualify_owned(
        &self,
        request: AbsentFinalAdoptionQualificationRequest,
    ) -> AbsentFinalAdoptionQualificationResult {
        let operation_id = request.operation_id;
        #[cfg(unix)]
        let outcome = qualify_unix_owned_absent_final_adoption(&request);
        #[cfg(not(unix))]
        let outcome = {
            let _ = request;
            AbsentFinalAdoptionOutcome::UnsupportedPlatform
        };
        AbsentFinalAdoptionQualificationResult {
            operation_id,
            outcome,
        }
    }
}

impl ProductionAbsentFinalAdoptionAdapter {
    /// Acquire a retained, operation-bound guard through the live target-parent capability.
    ///
    /// This is the file-operation owner boundary: the journal request contains only durable
    /// locators and expectations, while this adapter reacquires descriptors and verifies live
    /// identity and exact content before retaining either descriptor.
    pub(super) fn acquire_guard(
        &self,
        request: AbsentFinalAdoptionGuardRequest,
    ) -> Result<QualifiedAbsentFinalAdoptionGuard, AbsentFinalAdoptionOutcome> {
        #[cfg(unix)]
        {
            return acquire_unix_absent_final_adoption_guard(request);
        }
        #[cfg(not(unix))]
        {
            let _ = request;
            Err(AbsentFinalAdoptionOutcome::UnsupportedPlatform)
        }
    }

    /// Consume a retained adoption guard into sealed, operation-bound publication evidence.
    ///
    /// The conversion is intentionally owned by the adapter boundary.  Callers cannot retain,
    /// clone, serialize, compare, or inspect the descriptors that backed the guard.
    pub(super) fn consume_guard_for_publication(
        &self,
        guard: QualifiedAbsentFinalAdoptionGuard,
        expected_operation_id: OperationId,
    ) -> Result<OperationBoundQualifiedAbsentFinalNoReplace, AbsentFinalAdoptionOutcome> {
        guard.into_operation_bound_publication(expected_operation_id)
    }
}

/// Execute the live absent-final qualification at the physical file-owner boundary.
///
/// The journal supplies only the owned request and expected operation identity.  Descriptor
/// acquisition and the consuming retained-capability revalidation stay in this module so the
/// coordinator cannot accidentally perform live filesystem work while assembling publication
/// evidence.
pub(in crate::native_app) fn acquire_absent_final_publication_guard(
    request: AbsentFinalAdoptionGuardRequest,
    expected_operation_id: OperationId,
) -> Result<OperationBoundQualifiedAbsentFinalNoReplace, AbsentFinalAdoptionOutcome> {
    let adapter = ProductionAbsentFinalAdoptionAdapter;
    let guard = adapter.acquire_guard(request)?;
    adapter.consume_guard_for_publication(guard, expected_operation_id)
}

#[cfg(unix)]
fn open_final_relative(
    target_parent: &File,
    final_leaf: &Path,
) -> Result<File, AbsentFinalAdoptionOutcome> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = CString::new(final_leaf.as_os_str().as_encoded_bytes())
        .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    let fd = unsafe {
        libc::openat(
            target_parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        return match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::NotFound => Err(AbsentFinalAdoptionOutcome::FinalMissing),
            _ => Err(AbsentFinalAdoptionOutcome::VerificationFailed),
        };
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn observe_final(
    final_file: &File,
) -> Result<(PreparedObjectIdentity, [u8; 32]), AbsentFinalAdoptionOutcome> {
    let metadata = final_file
        .metadata()
        .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    if !metadata.is_file() {
        return Err(AbsentFinalAdoptionOutcome::VerificationFailed);
    }
    let identity = super::operation_journal::descriptor_identity(final_file)
        .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    let content = super::operation_journal::prepared_file_evidence(final_file);
    let PreparedFileEvidence::ContentHash(hash) = content else {
        return Err(AbsentFinalAdoptionOutcome::FinalContentDrift);
    };

    let after_identity = super::operation_journal::descriptor_identity(final_file)
        .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    if after_identity.stable_id != identity.stable_id || after_identity.len != identity.len {
        return Err(AbsentFinalAdoptionOutcome::FinalIdentityDrift);
    }
    Ok((after_identity, hash))
}

#[cfg(all(unix, test))]
fn qualify_unix_absent_final_adoption(
    request: AbsentFinalAdoptionRequest<'_>,
) -> AbsentFinalAdoptionOutcome {
    let target_parent_identity =
        match super::operation_journal::descriptor_identity(request.target_parent) {
            Ok(identity) if identity.stable_id == request.expected_target_parent.stable_id => {
                identity
            }
            Ok(_) => return AbsentFinalAdoptionOutcome::ParentIdentityDrift,
            Err(_) => return AbsentFinalAdoptionOutcome::VerificationFailed,
        };
    let final_object = match open_final_relative(request.target_parent, request.final_leaf) {
        Ok(file) => file,
        Err(outcome) => return outcome,
    };
    let (final_identity, final_content) = match observe_final(&final_object) {
        Ok(observation) => observation,
        Err(outcome) => return outcome,
    };
    if final_identity.stable_id != request.expected_final_stable_id
        || final_identity.len != request.expected_final_len
    {
        return AbsentFinalAdoptionOutcome::FinalIdentityDrift;
    }
    if &final_content != request.expected_final_content {
        return AbsentFinalAdoptionOutcome::FinalContentDrift;
    }
    AbsentFinalAdoptionOutcome::Qualified(QualifiedAbsentFinalAdoption {
        target_parent: target_parent_identity,
        final_object: final_identity,
        final_content: PreparedFileEvidence::ContentHash(final_content),
    })
}

#[cfg(all(unix, test))]
fn qualify_unix_owned_absent_final_adoption(
    request: &AbsentFinalAdoptionQualificationRequest,
) -> AbsentFinalAdoptionOutcome {
    let (target_parent, target_capability) =
        match super::operation_journal::open_root(&request.target_parent_path) {
            Ok(value) => value,
            Err(_) => return AbsentFinalAdoptionOutcome::VerificationFailed,
        };
    if target_capability.identity.stable_id != request.expected_target_parent_stable_id {
        return AbsentFinalAdoptionOutcome::ParentIdentityDrift;
    }
    let borrowed_request = match AbsentFinalAdoptionRequest::try_new(
        &target_parent,
        &request.final_leaf,
        &target_capability.identity,
        &request.expected_final_stable_id,
        request.expected_final_len,
        &request.expected_final_content,
    ) {
        Ok(request) => request,
        Err(_) => return AbsentFinalAdoptionOutcome::VerificationFailed,
    };
    qualify_unix_absent_final_adoption(borrowed_request)
}

#[cfg(unix)]
fn acquire_unix_absent_final_adoption_guard(
    request: AbsentFinalAdoptionGuardRequest,
) -> Result<QualifiedAbsentFinalAdoptionGuard, AbsentFinalAdoptionOutcome> {
    let (target_parent, target_capability) =
        super::operation_journal::open_root(&request.target_parent_path)
            .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    if target_capability.identity.stable_id != request.expected_target_parent_stable_id {
        return Err(AbsentFinalAdoptionOutcome::ParentIdentityDrift);
    }
    let final_request = AbsentFinalAdoptionRequest::try_new(
        &target_parent,
        &request.final_leaf,
        &target_capability.identity,
        &request.expected_final_stable_id,
        request.expected_final_len,
        &request.expected_final_content,
    )
    .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    let final_object = open_final_relative(&target_parent, &request.final_leaf)?;
    let (final_identity, final_content) = observe_final(&final_object)?;
    if final_identity.stable_id != final_request.expected_final_stable_id
        || final_identity.len != final_request.expected_final_len
    {
        return Err(AbsentFinalAdoptionOutcome::FinalIdentityDrift);
    }
    if final_content != request.expected_final_content {
        return Err(AbsentFinalAdoptionOutcome::FinalContentDrift);
    }
    Ok(QualifiedAbsentFinalAdoptionGuard {
        operation_id: request.operation_id,
        target_parent,
        final_object,
        final_leaf: request.final_leaf,
        target_parent_identity: target_capability.identity,
        final_identity,
        final_content,
    })
}

#[cfg(unix)]
fn revalidate_unix_absent_final_adoption_guard(
    guard: &QualifiedAbsentFinalAdoptionGuard,
) -> Result<(), AbsentFinalAdoptionOutcome> {
    let target_parent_identity =
        super::operation_journal::descriptor_identity(&guard.target_parent)
            .map_err(|_| AbsentFinalAdoptionOutcome::VerificationFailed)?;
    if target_parent_identity.stable_id != guard.target_parent_identity.stable_id {
        return Err(AbsentFinalAdoptionOutcome::ParentIdentityDrift);
    }

    let (retained_identity, retained_content) = observe_final(&guard.final_object)?;
    if retained_identity.stable_id != guard.final_identity.stable_id
        || retained_identity.len != guard.final_identity.len
    {
        return Err(AbsentFinalAdoptionOutcome::FinalIdentityDrift);
    }
    if retained_content != guard.final_content {
        return Err(AbsentFinalAdoptionOutcome::FinalContentDrift);
    }

    let final_object = open_final_relative(&guard.target_parent, &guard.final_leaf)?;
    let (final_identity, final_content) = observe_final(&final_object)?;
    if final_identity.stable_id != guard.final_identity.stable_id
        || final_identity.len != guard.final_identity.len
        || final_identity.stable_id != retained_identity.stable_id
        || final_identity.len != retained_identity.len
    {
        return Err(AbsentFinalAdoptionOutcome::FinalIdentityDrift);
    }
    if final_content != guard.final_content || final_content != retained_content {
        return Err(AbsentFinalAdoptionOutcome::FinalContentDrift);
    }
    Ok(())
}

impl<'a> AbsentFinalNoReplaceRequest<'a> {
    /// Build a request after validating both namespace leaves without touching the filesystem.
    pub(super) fn try_new(
        target_parent: &'a File,
        staging: &'a File,
        target_leaf: &'a Path,
        staging_leaf: &'a Path,
        expected_target_parent: &'a PreparedObjectIdentity,
        expected_staging: &'a PreparedObjectIdentity,
        expected_content: &'a PreparedFileEvidence,
    ) -> Result<Self, AbsentFinalNoReplaceRequestError> {
        if !is_single_clean_normal_leaf(target_leaf) {
            return Err(AbsentFinalNoReplaceRequestError::TargetLeafNotSingleCleanNormal);
        }
        if !is_single_clean_normal_leaf(staging_leaf) {
            return Err(AbsentFinalNoReplaceRequestError::StagingLeafNotSingleCleanNormal);
        }
        Ok(Self {
            target_parent,
            staging,
            target_leaf,
            staging_leaf,
            expected_target_parent,
            expected_staging,
            expected_content,
        })
    }
}

fn is_single_clean_normal_leaf(path: &Path) -> bool {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return false;
    }
    let mut components = path.components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return false;
    };
    if components.next().is_some() || Path::new(component) != path {
        return false;
    }
    !component.as_encoded_bytes().contains(&0)
}

/// Compare an observed object with durable identity while allowing a namespace operation to
/// refresh the change marker. Recovery uses this predicate before exact content matching.
pub(super) fn stable_id_and_length_matches(
    actual: &PreparedObjectIdentity,
    expected: &PreparedObjectIdentity,
) -> bool {
    actual.stable_id == expected.stable_id && actual.len == expected.len
}

/// Compare only exact content evidence. Metadata-only and unverifiable evidence never qualifies.
pub(super) fn exact_content_evidence_matches(
    expected: &PreparedFileEvidence,
    actual: &PreparedFileEvidence,
) -> bool {
    matches!(
        (expected, actual),
        (PreparedFileEvidence::ContentHash(expected), PreparedFileEvidence::ContentHash(actual))
            if expected == actual
    )
}

/// Collision observed while claiming the absent final name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AbsentFinalNoReplaceCollision {
    FinalEntryExists,
}

/// A platform or filesystem condition that requires a later qualification pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AbsentFinalNoReplaceQualification {
    MacosPrimitiveRequiresQualification,
    PrimitiveUnsupportedOnFilesystem,
    UnsupportedPlatform,
}

/// A drift or verification failure that cannot establish ownership of the final entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AbsentFinalNoReplaceFailure {
    TargetParentCapabilityDrift,
    StagingIdentityDrift,
    StagingContentDrift,
    NamespaceInspectionFailed,
    MutationFailed,
    ReopenFailed,
    ReopenedIdentityDrift,
    ReopenedContentDrift,
}

/// No claim is made about continuity of the configured root pathname.
///
/// The retained target-parent descriptor is the only namespace authority for this seam.  This
/// explicit value prevents `VisibilityVerified` from being interpreted as a pathname claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) enum RootPathContinuity {
    NotClaimed,
}

/// Result of one absent-final no-replace attempt.
pub(super) enum AbsentFinalNoReplaceOutcome {
    QualifiedSuccess(QualifiedAbsentFinalNoReplace),
    Collision(AbsentFinalNoReplaceCollision),
    PlatformQualificationRequired {
        reason: AbsentFinalNoReplaceQualification,
    },
    DriftOrVerificationFailure(AbsentFinalNoReplaceFailure),
}

/// Sealed evidence proving that a final entry was installed and reopened through the capability.
///
/// This type intentionally contains no whole-publication atomicity claim.  Synchronization is
/// retained as a separate, downgraded value and does not imply power-loss durability.
pub(super) struct QualifiedAbsentFinalNoReplace {
    target_parent_identity: PreparedObjectIdentity,
    root_path_continuity: RootPathContinuity,
    reopened_final: PreparedObjectIdentity,
    reopened_content: PreparedFileEvidence,
    visibility: PublicationVisibility,
    synchronization: PublicationSynchronization,
}

/// Sealed publication evidence that remains operation-bound until the journal verifies the ID.
///
/// This type deliberately has no descriptor, pathname, serialization, comparison, or clone
/// boundary.  The journal may only obtain the typed publication evidence by consuming it with the
/// expected operation ID after its exact record snapshot has been checked.
pub(in crate::native_app) struct OperationBoundQualifiedAbsentFinalNoReplace {
    operation_id: OperationId,
    qualified: QualifiedAbsentFinalNoReplace,
}

impl OperationBoundQualifiedAbsentFinalNoReplace {
    pub(super) fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    pub(super) fn into_qualified(
        self,
        expected_operation_id: OperationId,
    ) -> Result<QualifiedAbsentFinalNoReplace, AbsentFinalAdoptionOutcome> {
        if self.operation_id != expected_operation_id {
            return Err(AbsentFinalAdoptionOutcome::OperationIdDrift);
        }
        Ok(self.qualified)
    }
}

impl QualifiedAbsentFinalNoReplace {
    /// Consume the sealed result into the later publication-evidence boundary.
    pub(super) fn into_publication_parts(
        self,
    ) -> (
        PreparedObjectIdentity,
        RootPathContinuity,
        PreparedObjectIdentity,
        PreparedFileEvidence,
        PublicationVisibility,
        PublicationSynchronization,
    ) {
        (
            self.target_parent_identity,
            self.root_path_continuity,
            self.reopened_final,
            self.reopened_content,
            self.visibility,
            self.synchronization,
        )
    }
}

/// Adapter boundary for an absent-final no-replace claim.
pub(super) trait AbsentFinalNoReplaceAdapter: sealed::Sealed {
    fn attempt(&self, request: AbsentFinalNoReplaceRequest<'_>) -> AbsentFinalNoReplaceOutcome;
}

/// Production adapter.  It performs no filesystem I/O and does not mutate the namespace.
pub(super) struct ProductionAbsentFinalNoReplaceAdapter;

impl sealed::Sealed for ProductionAbsentFinalNoReplaceAdapter {}

impl AbsentFinalNoReplaceAdapter for ProductionAbsentFinalNoReplaceAdapter {
    fn attempt(&self, _request: AbsentFinalNoReplaceRequest<'_>) -> AbsentFinalNoReplaceOutcome {
        #[cfg(target_os = "macos")]
        let reason = AbsentFinalNoReplaceQualification::MacosPrimitiveRequiresQualification;
        #[cfg(not(target_os = "macos"))]
        let reason = AbsentFinalNoReplaceQualification::UnsupportedPlatform;
        AbsentFinalNoReplaceOutcome::PlatformQualificationRequired { reason }
    }
}

#[cfg(all(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TestMutationFault {
    None,
    CreateDestinationBeforeClaim,
    RemoveDestinationBeforeReopen,
}

#[cfg(all(target_os = "macos", test))]
const MACOS_RENAME_NOFOLLOW_ANY: libc::c_uint = 0x0000_0010;

#[cfg(all(target_os = "macos", test))]
const MACOS_RENAME_RESOLVE_BENEATH: libc::c_uint = 0x0000_0020;

/// Test-only native attempt.  An optional hook can replace the configured root pathname between
/// the claim and reopen; namespace authority remains the retained `target_parent` descriptor.
#[cfg(all(target_os = "macos", test))]
pub(super) fn test_mutating_attempt(
    request: AbsentFinalNoReplaceRequest<'_>,
    fault: TestMutationFault,
    mut after_claim: Option<&mut dyn FnMut() -> Result<(), ()>>,
) -> AbsentFinalNoReplaceOutcome {
    use super::operation_journal::{open_leaf_relative, prepared_file_evidence};

    let target_parent_identity = match validate_target_parent_capability(&request) {
        Ok(identity) => identity,
        Err(failure) => {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(failure);
        }
    };
    if let Err(failure) = verify_staging(&request) {
        return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(failure);
    }
    match target_entry_exists(&request) {
        Ok(true) => {
            return AbsentFinalNoReplaceOutcome::Collision(
                AbsentFinalNoReplaceCollision::FinalEntryExists,
            );
        }
        Ok(false) => {}
        Err(failure) => {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(failure);
        }
    }

    if fault == TestMutationFault::CreateDestinationBeforeClaim
        && create_relative_file(request.target_parent, request.target_leaf).is_err()
    {
        return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
            AbsentFinalNoReplaceFailure::MutationFailed,
        );
    }

    let source_name = match leaf_c_string(request.staging_leaf) {
        Ok(name) => name,
        Err(_) => {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::StagingIdentityDrift,
            );
        }
    };
    let target_name = match leaf_c_string(request.target_leaf) {
        Ok(name) => name,
        Err(_) => {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::NamespaceInspectionFailed,
            );
        }
    };
    let target_parent_fd = std::os::fd::AsRawFd::as_raw_fd(request.target_parent);
    let flags = libc::RENAME_EXCL | MACOS_RENAME_NOFOLLOW_ANY | MACOS_RENAME_RESOLVE_BENEATH;
    let result = unsafe {
        libc::renameatx_np(
            target_parent_fd,
            source_name.as_ptr(),
            target_parent_fd,
            target_name.as_ptr(),
            flags,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::EEXIST) => AbsentFinalNoReplaceOutcome::Collision(
                AbsentFinalNoReplaceCollision::FinalEntryExists,
            ),
            Some(libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP) => {
                AbsentFinalNoReplaceOutcome::PlatformQualificationRequired {
                    reason: AbsentFinalNoReplaceQualification::PrimitiveUnsupportedOnFilesystem,
                }
            }
            _ => AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::MutationFailed,
            ),
        };
    }

    if let Err(failure) = validate_target_parent_capability(&request) {
        return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(failure);
    }
    if let Some(after_claim) = after_claim.as_mut() {
        if after_claim().is_err() {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::MutationFailed,
            );
        }
    }
    if fault == TestMutationFault::RemoveDestinationBeforeReopen {
        let result = unsafe { libc::unlinkat(target_parent_fd, target_name.as_ptr(), 0) };
        if result != 0 {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::MutationFailed,
            );
        }
    }

    let (reopened_final, reopened_identity) = match open_leaf_relative(
        request.target_parent,
        request.target_leaf,
        request.target_leaf,
    ) {
        Ok(value) => value,
        Err(_) => {
            return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::ReopenFailed,
            );
        }
    };
    if !stable_id_and_length_matches(&reopened_identity, request.expected_staging) {
        return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
            AbsentFinalNoReplaceFailure::ReopenedIdentityDrift,
        );
    }
    let reopened_content = prepared_file_evidence(&reopened_final);
    if !exact_content_evidence_matches(request.expected_content, &reopened_content) {
        return AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
            AbsentFinalNoReplaceFailure::ReopenedContentDrift,
        );
    }
    AbsentFinalNoReplaceOutcome::QualifiedSuccess(QualifiedAbsentFinalNoReplace {
        target_parent_identity,
        root_path_continuity: RootPathContinuity::NotClaimed,
        reopened_final: reopened_identity,
        reopened_content,
        visibility: PublicationVisibility::VisibilityVerified,
        synchronization: PublicationSynchronization::SyncUnsupportedOrUnverified,
    })
}

#[cfg(all(target_os = "macos", test))]
fn validate_target_parent_capability(
    request: &AbsentFinalNoReplaceRequest<'_>,
) -> Result<PreparedObjectIdentity, AbsentFinalNoReplaceFailure> {
    use super::operation_journal::descriptor_identity;

    let retained_identity = descriptor_identity(request.target_parent)
        .map_err(|_| AbsentFinalNoReplaceFailure::TargetParentCapabilityDrift)?;
    if retained_identity.stable_id != request.expected_target_parent.stable_id {
        return Err(AbsentFinalNoReplaceFailure::TargetParentCapabilityDrift);
    }
    Ok(request.expected_target_parent.clone())
}

#[cfg(all(target_os = "macos", test))]
fn verify_staging(
    request: &AbsentFinalNoReplaceRequest<'_>,
) -> Result<(), AbsentFinalNoReplaceFailure> {
    use super::operation_journal::{
        descriptor_identity, open_leaf_relative, prepared_file_evidence,
    };

    let identity = descriptor_identity(request.staging)
        .map_err(|_| AbsentFinalNoReplaceFailure::StagingIdentityDrift)?;
    if identity != *request.expected_staging {
        return Err(AbsentFinalNoReplaceFailure::StagingIdentityDrift);
    }
    let content = prepared_file_evidence(request.staging);
    if !exact_content_evidence_matches(request.expected_content, &content) {
        return Err(AbsentFinalNoReplaceFailure::StagingContentDrift);
    }

    let (_, relative_identity) = open_leaf_relative(
        request.target_parent,
        request.staging_leaf,
        request.staging_leaf,
    )
    .map_err(|_| AbsentFinalNoReplaceFailure::StagingIdentityDrift)?;
    if relative_identity != *request.expected_staging {
        return Err(AbsentFinalNoReplaceFailure::StagingIdentityDrift);
    }
    let (relative_staging, _) = open_leaf_relative(
        request.target_parent,
        request.staging_leaf,
        request.staging_leaf,
    )
    .map_err(|_| AbsentFinalNoReplaceFailure::StagingIdentityDrift)?;
    if !exact_content_evidence_matches(
        request.expected_content,
        &prepared_file_evidence(&relative_staging),
    ) {
        return Err(AbsentFinalNoReplaceFailure::StagingContentDrift);
    }
    Ok(())
}

#[cfg(all(target_os = "macos", test))]
fn target_entry_exists(
    request: &AbsentFinalNoReplaceRequest<'_>,
) -> Result<bool, AbsentFinalNoReplaceFailure> {
    use std::os::fd::AsRawFd;

    let target_name = leaf_c_string(request.target_leaf)
        .map_err(|_| AbsentFinalNoReplaceFailure::NamespaceInspectionFailed)?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let result = unsafe {
        libc::fstatat(
            request.target_parent.as_raw_fd(),
            target_name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        return Ok(true);
    }
    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        Ok(false)
    } else {
        Err(AbsentFinalNoReplaceFailure::NamespaceInspectionFailed)
    }
}

#[cfg(all(target_os = "macos", test))]
fn leaf_c_string(path: &Path) -> Result<std::ffi::CString, ()> {
    std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| ())
}

#[cfg(all(target_os = "macos", test))]
fn create_relative_file(parent: &File, leaf: &Path) -> Result<(), ()> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = leaf_c_string(leaf)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(());
    }
    let _file = unsafe { File::from_raw_fd(fd) };
    Ok(())
}

#[cfg(all(target_os = "macos", test))]
fn rename_directory_relative(parent: &File, source: &Path, destination: &Path) -> Result<(), ()> {
    use std::os::fd::AsRawFd;

    let source_name = leaf_c_string(source)?;
    let destination_name = leaf_c_string(destination)?;
    let result = unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            source_name.as_ptr(),
            parent.as_raw_fd(),
            destination_name.as_ptr(),
        )
    };
    if result == 0 { Ok(()) } else { Err(()) }
}

#[cfg(test)]
pub(super) fn test_qualified_success(
    target_parent_identity: &PreparedObjectIdentity,
    reopened_final: &PreparedObjectIdentity,
    reopened_content: &PreparedFileEvidence,
) -> Option<QualifiedAbsentFinalNoReplace> {
    if !matches!(reopened_content, PreparedFileEvidence::ContentHash(_)) {
        return None;
    }
    Some(QualifiedAbsentFinalNoReplace {
        target_parent_identity: target_parent_identity.clone(),
        root_path_continuity: RootPathContinuity::NotClaimed,
        reopened_final: reopened_final.clone(),
        reopened_content: reopened_content.clone(),
        visibility: PublicationVisibility::VisibilityVerified,
        synchronization: PublicationSynchronization::SyncUnsupportedOrUnverified,
    })
}

#[cfg(test)]
mod tests {
    use super::super::operation_journal::{descriptor_identity, prepared_file_evidence};
    use super::*;
    use std::fs;
    use tempfile::{TempDir, tempdir};

    const STAGING_LEAF: &str = "staging.wav";
    const TARGET_LEAF: &str = "final.wav";
    const STAGING_BYTES: &[u8] = b"prepared staging bytes";

    struct Fixture {
        _directory: TempDir,
        #[cfg(target_os = "macos")]
        _base_parent: File,
        target_parent_path: std::path::PathBuf,
        target_parent: File,
        staging: File,
        target_parent_identity: PreparedObjectIdentity,
        expected_staging: PreparedObjectIdentity,
        expected_content: PreparedFileEvidence,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempdir().expect("isolated fixture directory");
            let base = directory
                .path()
                .canonicalize()
                .expect("canonical fixture directory");
            #[cfg(target_os = "macos")]
            let base_parent = File::open(&base).expect("fixture base parent handle");
            let target_parent_path = base.join("target-parent");
            fs::create_dir(&target_parent_path).expect("target parent");
            let staging_path = target_parent_path.join(STAGING_LEAF);
            fs::write(&staging_path, STAGING_BYTES).expect("staging bytes");
            let target_parent = File::open(&target_parent_path).expect("target parent handle");
            let staging = File::open(&staging_path).expect("staging handle");
            let target_parent_identity =
                descriptor_identity(&target_parent).expect("target parent identity");
            let expected_staging = descriptor_identity(&staging).expect("staging identity");
            let expected_content = prepared_file_evidence(&staging);
            Self {
                _directory: directory,
                #[cfg(target_os = "macos")]
                _base_parent: base_parent,
                target_parent_path,
                target_parent,
                staging,
                target_parent_identity,
                expected_staging,
                expected_content,
            }
        }

        fn request(&self) -> AbsentFinalNoReplaceRequest<'_> {
            AbsentFinalNoReplaceRequest::try_new(
                &self.target_parent,
                &self.staging,
                Path::new(TARGET_LEAF),
                Path::new(STAGING_LEAF),
                &self.target_parent_identity,
                &self.expected_staging,
                &self.expected_content,
            )
            .expect("valid absent-final request")
        }

        fn target_path(&self) -> std::path::PathBuf {
            self.target_parent_path.join(TARGET_LEAF)
        }

        fn staging_path(&self) -> std::path::PathBuf {
            self.target_parent_path.join(STAGING_LEAF)
        }

        #[cfg(target_os = "macos")]
        fn replace_root_path(&self) -> std::path::PathBuf {
            let moved = self
                .target_parent_path
                .parent()
                .expect("fixture parent")
                .join("moved-target-parent");
            rename_directory_relative(
                &self._base_parent,
                Path::new("target-parent"),
                Path::new("moved-target-parent"),
            )
            .expect("replace target root path");
            fs::create_dir(&self.target_parent_path).expect("replacement target root");
            moved
        }
    }

    #[test]
    fn request_rejects_non_single_clean_normal_leaves() {
        let fixture = Fixture::new();
        assert!(matches!(
            AbsentFinalNoReplaceRequest::try_new(
                &fixture.target_parent,
                &fixture.staging,
                Path::new("nested/final.wav"),
                Path::new(STAGING_LEAF),
                &fixture.target_parent_identity,
                &fixture.expected_staging,
                &fixture.expected_content,
            ),
            Err(error) if error == AbsentFinalNoReplaceRequestError::TargetLeafNotSingleCleanNormal
        ));
        assert!(matches!(
            AbsentFinalNoReplaceRequest::try_new(
                &fixture.target_parent,
                &fixture.staging,
                Path::new(TARGET_LEAF),
                Path::new("../staging.wav"),
                &fixture.target_parent_identity,
                &fixture.expected_staging,
                &fixture.expected_content,
            ),
            Err(error) if error == AbsentFinalNoReplaceRequestError::StagingLeafNotSingleCleanNormal
        ));
    }

    #[test]
    fn production_adapter_is_fail_closed_and_does_not_mutate() {
        let fixture = Fixture::new();
        let outcome = ProductionAbsentFinalNoReplaceAdapter.attempt(fixture.request());
        let expected_reason = if cfg!(target_os = "macos") {
            AbsentFinalNoReplaceQualification::MacosPrimitiveRequiresQualification
        } else {
            AbsentFinalNoReplaceQualification::UnsupportedPlatform
        };
        assert!(matches!(
            outcome,
            AbsentFinalNoReplaceOutcome::PlatformQualificationRequired { reason }
                if reason == expected_reason
        ));
        assert_eq!(
            fs::read(fixture.staging_path()).expect("staging remains"),
            STAGING_BYTES
        );
        assert!(!fixture.target_path().exists());
    }

    #[cfg(all(target_os = "macos", test))]
    fn native_attempt(fixture: &Fixture) -> AbsentFinalNoReplaceOutcome {
        test_mutating_attempt(fixture.request(), TestMutationFault::None, None)
    }

    #[cfg(all(target_os = "macos", test))]
    fn assert_not_qualified(outcome: &AbsentFinalNoReplaceOutcome) {
        assert!(!matches!(
            outcome,
            AbsentFinalNoReplaceOutcome::QualifiedSuccess(_)
        ));
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn macos_qualified_success_reopens_through_retained_parent_capability() {
        let fixture = Fixture::new();
        let outcome = native_attempt(&fixture);
        let AbsentFinalNoReplaceOutcome::QualifiedSuccess(qualified) = outcome else {
            panic!("macOS/native no-replace qualification did not succeed");
        };
        assert_eq!(
            qualified.reopened_final.stable_id,
            fixture.expected_staging.stable_id
        );
        assert_eq!(qualified.reopened_final.len, fixture.expected_staging.len);
        assert_eq!(qualified.reopened_content, fixture.expected_content);
        assert_eq!(
            qualified.target_parent_identity,
            fixture.target_parent_identity
        );
        assert_eq!(
            qualified.root_path_continuity,
            RootPathContinuity::NotClaimed
        );
        assert_eq!(
            qualified.visibility,
            PublicationVisibility::VisibilityVerified
        );
        assert_eq!(
            qualified.synchronization,
            PublicationSynchronization::SyncUnsupportedOrUnverified
        );
        assert_eq!(fs::read(fixture.target_path()).unwrap(), STAGING_BYTES);
        assert!(!fixture.staging_path().exists());
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn existing_and_late_final_entries_are_collisions() {
        let existing = Fixture::new();
        fs::write(existing.target_path(), b"existing").expect("existing final");
        assert!(matches!(
            native_attempt(&existing),
            AbsentFinalNoReplaceOutcome::Collision(AbsentFinalNoReplaceCollision::FinalEntryExists)
        ));
        assert_eq!(fs::read(existing.staging_path()).unwrap(), STAGING_BYTES);

        let late = Fixture::new();
        let outcome = test_mutating_attempt(
            late.request(),
            TestMutationFault::CreateDestinationBeforeClaim,
            None,
        );
        assert!(matches!(
            outcome,
            AbsentFinalNoReplaceOutcome::Collision(AbsentFinalNoReplaceCollision::FinalEntryExists)
        ));
        assert_eq!(fs::read(late.staging_path()).unwrap(), STAGING_BYTES);
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn symlink_final_is_a_collision_and_is_never_followed() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        symlink(STAGING_LEAF, fixture.target_path()).expect("symlink final");
        let outcome = native_attempt(&fixture);
        assert!(matches!(
            outcome,
            AbsentFinalNoReplaceOutcome::Collision(AbsentFinalNoReplaceCollision::FinalEntryExists)
        ));
        assert!(fixture.target_path().is_symlink());
        assert_eq!(fs::read(fixture.staging_path()).unwrap(), STAGING_BYTES);
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn replaced_root_path_before_claim_keeps_qualification_in_moved_namespace() {
        let fixture = Fixture::new();
        let moved = fixture.replace_root_path();
        let outcome = native_attempt(&fixture);
        let AbsentFinalNoReplaceOutcome::QualifiedSuccess(qualified) = outcome else {
            panic!("pathname replacement must not redirect descriptor-bound qualification");
        };
        assert_eq!(
            qualified.target_parent_identity,
            fixture.target_parent_identity
        );
        assert_eq!(
            qualified.root_path_continuity,
            RootPathContinuity::NotClaimed
        );
        assert_eq!(fs::read(moved.join(TARGET_LEAF)).unwrap(), STAGING_BYTES);
        assert!(!moved.join(STAGING_LEAF).exists());
        assert!(!fixture.target_path().exists());
        assert!(!fixture.staging_path().exists());
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn replaced_root_path_after_claim_before_reopen_keeps_qualification_in_moved_namespace() {
        let fixture = Fixture::new();
        let mut replace_root_path = || {
            fixture.replace_root_path();
            Ok(())
        };
        let outcome = test_mutating_attempt(
            fixture.request(),
            TestMutationFault::None,
            Some(&mut replace_root_path),
        );
        let AbsentFinalNoReplaceOutcome::QualifiedSuccess(qualified) = outcome else {
            panic!("reopen must remain bound to the retained target-parent descriptor");
        };
        assert_eq!(
            qualified.target_parent_identity,
            fixture.target_parent_identity
        );
        assert_eq!(
            qualified.root_path_continuity,
            RootPathContinuity::NotClaimed
        );
        let moved = fixture
            .target_parent_path
            .parent()
            .expect("fixture parent")
            .join("moved-target-parent");
        assert_eq!(fs::read(moved.join(TARGET_LEAF)).unwrap(), STAGING_BYTES);
        assert!(!fixture.target_path().exists());
        assert!(!fixture.staging_path().exists());
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn mismatched_expected_target_parent_identity_fails_closed_without_mutation() {
        let fixture = Fixture::new();
        let mut wrong_identity = fixture.target_parent_identity.clone();
        wrong_identity.stable_id = String::from("wrong-target-parent");
        let request = AbsentFinalNoReplaceRequest::try_new(
            &fixture.target_parent,
            &fixture.staging,
            Path::new(TARGET_LEAF),
            Path::new(STAGING_LEAF),
            &wrong_identity,
            &fixture.expected_staging,
            &fixture.expected_content,
        )
        .expect("valid leaf request");
        let outcome = test_mutating_attempt(request, TestMutationFault::None, None);
        assert!(matches!(
            outcome,
            AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::TargetParentCapabilityDrift
            )
        ));
        assert_eq!(fs::read(fixture.staging_path()).unwrap(), STAGING_BYTES);
        assert!(!fixture.target_path().exists());
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn staging_identity_drift_is_not_qualified() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.staging_path()).expect("remove staged entry");
        fs::write(fixture.staging_path(), b"replacement staging").expect("replacement staging");
        let outcome = native_attempt(&fixture);
        assert!(matches!(
            &outcome,
            AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::StagingIdentityDrift
            )
        ));
        assert_not_qualified(&outcome);
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn staging_content_drift_is_not_qualified() {
        let fixture = Fixture::new();
        fs::write(fixture.staging_path(), b"changed staging bytes").expect("changed staging");
        let changed_identity = descriptor_identity(&fixture.staging).expect("changed identity");
        let request = AbsentFinalNoReplaceRequest::try_new(
            &fixture.target_parent,
            &fixture.staging,
            Path::new(TARGET_LEAF),
            Path::new(STAGING_LEAF),
            &fixture.target_parent_identity,
            &changed_identity,
            &fixture.expected_content,
        )
        .expect("valid drift request");
        let outcome = test_mutating_attempt(request, TestMutationFault::None, None);
        assert!(matches!(
            &outcome,
            AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::StagingContentDrift
            )
        ));
        assert_not_qualified(&outcome);
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn unverifiable_staging_content_requires_qualification() {
        let fixture = Fixture::new();
        let unverifiable = PreparedFileEvidence::Unverifiable;
        let request = AbsentFinalNoReplaceRequest::try_new(
            &fixture.target_parent,
            &fixture.staging,
            Path::new(TARGET_LEAF),
            Path::new(STAGING_LEAF),
            &fixture.target_parent_identity,
            &fixture.expected_staging,
            &unverifiable,
        )
        .expect("valid leaf request");
        let outcome = test_mutating_attempt(request, TestMutationFault::None, None);
        assert!(matches!(
            &outcome,
            AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::StagingContentDrift
            )
        ));
        assert_not_qualified(&outcome);
        assert!(!fixture.target_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn adoption_qualifies_final_after_staging_to_final_rename() {
        let fixture = Fixture::new();
        fs::rename(fixture.staging_path(), fixture.target_path()).expect("stage final");
        let final_file = File::open(fixture.target_path()).expect("final file");
        let final_identity = descriptor_identity(&final_file).expect("final identity");
        let request = AbsentFinalAdoptionRequest::try_new(
            &fixture.target_parent,
            Path::new(TARGET_LEAF),
            &fixture.target_parent_identity,
            &final_identity.stable_id,
            final_identity.len,
            match &fixture.expected_content {
                PreparedFileEvidence::ContentHash(hash) => hash,
                _ => panic!("fixture must have exact content"),
            },
        )
        .expect("valid adoption request");
        let outcome = ProductionAbsentFinalAdoptionAdapter.qualify(request);
        assert!(matches!(outcome, AbsentFinalAdoptionOutcome::Qualified(_)));
        assert!(!fixture.staging_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn owned_adoption_qualification_reacquires_and_returns_operation_bound_evidence() {
        let fixture = Fixture::new();
        fs::rename(fixture.staging_path(), fixture.target_path()).expect("stage final");
        let final_file = File::open(fixture.target_path()).expect("final file");
        let final_identity = descriptor_identity(&final_file).expect("final identity");
        let expected_content = match &fixture.expected_content {
            PreparedFileEvidence::ContentHash(hash) => *hash,
            _ => panic!("fixture must have exact content"),
        };
        let operation_id = OperationId::for_test();
        let request = AbsentFinalAdoptionQualificationRequest::try_new(
            operation_id,
            fixture.target_parent_path.clone(),
            PathBuf::from(TARGET_LEAF),
            fixture.target_parent_identity.stable_id.clone(),
            final_identity.stable_id,
            final_identity.len,
            expected_content,
        )
        .expect("owned adoption request");
        let result = ProductionAbsentFinalAdoptionAdapter.qualify_owned(request);

        assert_eq!(result.operation_id(), operation_id);
        assert!(matches!(
            result.outcome,
            AbsentFinalAdoptionOutcome::Qualified(_)
        ));
        assert!(!fixture.staging_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn publication_guard_consuming_conversion_revalidates_retained_final() {
        let fixture = Fixture::new();
        fs::rename(fixture.staging_path(), fixture.target_path()).expect("stage final");
        let final_file = File::open(fixture.target_path()).expect("final file");
        let final_identity = descriptor_identity(&final_file).expect("final identity");
        let expected_content = match &fixture.expected_content {
            PreparedFileEvidence::ContentHash(hash) => *hash,
            _ => panic!("fixture must have exact content"),
        };
        let operation_id = OperationId::for_test();
        let request = AbsentFinalAdoptionGuardRequest::try_new(
            operation_id,
            fixture.target_parent_path.clone(),
            PathBuf::from(TARGET_LEAF),
            fixture.target_parent_identity.stable_id.clone(),
            final_identity.stable_id,
            final_identity.len,
            expected_content,
        )
        .expect("publication guard request");
        let guard = ProductionAbsentFinalAdoptionAdapter
            .acquire_guard(request)
            .expect("acquire publication guard");

        fs::write(fixture.target_path(), b"prepared staging byte!")
            .expect("mutate retained final with equal-length content");
        assert!(matches!(
            ProductionAbsentFinalAdoptionAdapter.consume_guard_for_publication(guard, operation_id),
            Err(AbsentFinalAdoptionOutcome::FinalContentDrift)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn owned_adoption_qualification_request_does_not_claim_missing_final() {
        let fixture = Fixture::new();
        let expected_content = match &fixture.expected_content {
            PreparedFileEvidence::ContentHash(hash) => *hash,
            _ => panic!("fixture must have exact content"),
        };
        let operation_id = OperationId::for_test();
        let request = AbsentFinalAdoptionQualificationRequest::try_new(
            operation_id,
            fixture.target_parent_path.clone(),
            PathBuf::from(TARGET_LEAF),
            fixture.target_parent_identity.stable_id.clone(),
            fixture.expected_staging.stable_id.clone(),
            fixture.expected_staging.len,
            expected_content,
        )
        .expect("owned adoption request without live inspection");
        let result = ProductionAbsentFinalAdoptionAdapter.qualify_owned(request);

        assert_eq!(result.operation_id(), operation_id);
        assert_eq!(result.outcome, AbsentFinalAdoptionOutcome::FinalMissing);
        assert!(!fixture.target_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn adoption_missing_final_and_replacement_fail_closed() {
        let fixture = Fixture::new();
        let hash = match &fixture.expected_content {
            PreparedFileEvidence::ContentHash(hash) => hash,
            _ => panic!("fixture must have exact content"),
        };
        let request = AbsentFinalAdoptionRequest::try_new(
            &fixture.target_parent,
            Path::new(TARGET_LEAF),
            &fixture.target_parent_identity,
            &fixture.expected_staging.stable_id,
            fixture.expected_staging.len,
            hash,
        )
        .expect("valid adoption request");
        assert_eq!(
            ProductionAbsentFinalAdoptionAdapter.qualify(request),
            AbsentFinalAdoptionOutcome::FinalMissing
        );
        fs::write(fixture.target_path(), b"replacement").expect("replacement final");
        let request = AbsentFinalAdoptionRequest::try_new(
            &fixture.target_parent,
            Path::new(TARGET_LEAF),
            &fixture.target_parent_identity,
            &fixture.expected_staging.stable_id,
            fixture.expected_staging.len,
            hash,
        )
        .expect("valid adoption request");
        assert!(matches!(
            ProductionAbsentFinalAdoptionAdapter.qualify(request),
            AbsentFinalAdoptionOutcome::FinalIdentityDrift
                | AbsentFinalAdoptionOutcome::FinalContentDrift
        ));
    }

    #[cfg(unix)]
    #[test]
    fn adoption_rejects_parent_identity_and_final_symlink() {
        let fixture = Fixture::new();
        let hash = match &fixture.expected_content {
            PreparedFileEvidence::ContentHash(hash) => hash,
            _ => panic!("fixture must have exact content"),
        };
        let mut wrong_parent = fixture.target_parent_identity.clone();
        wrong_parent.stable_id = String::from("wrong-parent");
        let request = AbsentFinalAdoptionRequest::try_new(
            &fixture.target_parent,
            Path::new(TARGET_LEAF),
            &wrong_parent,
            &fixture.expected_staging.stable_id,
            fixture.expected_staging.len,
            hash,
        )
        .expect("valid adoption request");
        assert_eq!(
            ProductionAbsentFinalAdoptionAdapter.qualify(request),
            AbsentFinalAdoptionOutcome::ParentIdentityDrift
        );
        std::os::unix::fs::symlink(fixture.staging_path(), fixture.target_path())
            .expect("symlink final");
        let request = AbsentFinalAdoptionRequest::try_new(
            &fixture.target_parent,
            Path::new(TARGET_LEAF),
            &fixture.target_parent_identity,
            &fixture.expected_staging.stable_id,
            fixture.expected_staging.len,
            hash,
        )
        .expect("valid adoption request");
        assert_eq!(
            ProductionAbsentFinalAdoptionAdapter.qualify(request),
            AbsentFinalAdoptionOutcome::VerificationFailed
        );
    }

    #[cfg(unix)]
    #[test]
    fn adoption_parent_reacquisition_rejects_symlink_path() {
        let fixture = Fixture::new();
        let alias = fixture
            .target_parent_path
            .parent()
            .expect("fixture parent")
            .join("target-parent-alias");
        std::os::unix::fs::symlink(&fixture.target_parent_path, &alias)
            .expect("symlink target parent");
        assert!(super::super::operation_journal::open_root(&alias).is_err());
    }

    #[cfg(all(target_os = "macos", test))]
    #[test]
    fn reopen_failure_is_not_qualified() {
        let fixture = Fixture::new();
        let outcome = test_mutating_attempt(
            fixture.request(),
            TestMutationFault::RemoveDestinationBeforeReopen,
            None,
        );
        assert!(matches!(
            &outcome,
            AbsentFinalNoReplaceOutcome::DriftOrVerificationFailure(
                AbsentFinalNoReplaceFailure::ReopenFailed
            )
        ));
        assert_not_qualified(&outcome);
        assert!(!fixture.target_path().exists());
    }
}
