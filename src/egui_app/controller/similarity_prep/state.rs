use super::controller_state::{SimilarityPrepStage, SimilarityPrepState};
use crate::sample_sources::SourceId;

pub(super) struct SimilarityPrepInit {
    pub(super) source_id: SourceId,
    pub(super) umap_version: String,
    pub(super) scan_completed_at: Option<i64>,
    pub(super) skip_scan: bool,
    pub(super) needs_embeddings: bool,
    pub(super) force_full_analysis: bool,
}

pub(super) struct SimilarityScanTransition {
    pub(super) force_full: bool,
    pub(super) should_enqueue_embeddings: bool,
}

pub(super) struct SimilarityFinalizeRequest {
    pub(super) source_id: SourceId,
    pub(super) umap_version: String,
}

pub(super) fn build_initial_state(init: SimilarityPrepInit) -> SimilarityPrepState {
    let stage = if init.skip_scan {
        SimilarityPrepStage::AwaitEmbeddings
    } else {
        SimilarityPrepStage::AwaitScan
    };
    SimilarityPrepState {
        source_id: init.source_id,
        stage,
        umap_version: init.umap_version,
        scan_completed_at: init.scan_completed_at,
        skip_backfill: init.skip_scan && !init.needs_embeddings && !init.force_full_analysis,
        force_full_analysis: init.force_full_analysis,
    }
}

pub(super) fn apply_scan_finished(
    state: &mut SimilarityPrepState,
    scan_completed_at: Option<i64>,
    scan_changed: bool,
) -> SimilarityScanTransition {
    state.stage = SimilarityPrepStage::AwaitEmbeddings;
    state.scan_completed_at = scan_completed_at;
    state.skip_backfill = !scan_changed && !state.force_full_analysis;
    SimilarityScanTransition {
        force_full: state.force_full_analysis,
        should_enqueue_embeddings: scan_changed || state.force_full_analysis,
    }
}

pub(super) fn start_finalize_if_ready(
    state: &mut SimilarityPrepState,
) -> Option<SimilarityFinalizeRequest> {
    if state.stage != SimilarityPrepStage::AwaitEmbeddings {
        return None;
    }
    state.stage = SimilarityPrepStage::Finalizing;
    Some(SimilarityFinalizeRequest {
        source_id: state.source_id.clone(),
        umap_version: state.umap_version.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_init(skip_scan: bool, needs_embeddings: bool, force_full: bool) -> SimilarityPrepInit {
        SimilarityPrepInit {
            source_id: SourceId::new(),
            umap_version: "v1".to_string(),
            scan_completed_at: Some(10),
            skip_scan,
            needs_embeddings,
            force_full_analysis: force_full,
        }
    }

    #[test]
    fn initial_state_sets_stage_and_skip_backfill() {
        let state = build_initial_state(build_init(false, false, false));
        assert_eq!(state.stage, SimilarityPrepStage::AwaitScan);
        assert!(!state.skip_backfill);

        let state = build_initial_state(build_init(true, false, false));
        assert_eq!(state.stage, SimilarityPrepStage::AwaitEmbeddings);
        assert!(state.skip_backfill);

        let state = build_initial_state(build_init(true, true, false));
        assert_eq!(state.stage, SimilarityPrepStage::AwaitEmbeddings);
        assert!(!state.skip_backfill);

        let state = build_initial_state(build_init(true, false, true));
        assert_eq!(state.stage, SimilarityPrepStage::AwaitEmbeddings);
        assert!(!state.skip_backfill);
    }

    #[test]
    fn scan_transition_controls_backfill() {
        let mut state = build_initial_state(build_init(false, false, false));
        let transition = apply_scan_finished(&mut state, Some(12), false);
        assert_eq!(state.stage, SimilarityPrepStage::AwaitEmbeddings);
        assert!(state.skip_backfill);
        assert!(!transition.should_enqueue_embeddings);
        assert!(!transition.force_full);

        let mut state = build_initial_state(build_init(false, false, true));
        let transition = apply_scan_finished(&mut state, Some(12), false);
        assert_eq!(state.stage, SimilarityPrepStage::AwaitEmbeddings);
        assert!(!state.skip_backfill);
        assert!(transition.should_enqueue_embeddings);
        assert!(transition.force_full);

        let mut state = build_initial_state(build_init(false, false, false));
        let transition = apply_scan_finished(&mut state, Some(12), true);
        assert_eq!(state.stage, SimilarityPrepStage::AwaitEmbeddings);
        assert!(!state.skip_backfill);
        assert!(transition.should_enqueue_embeddings);
    }

    #[test]
    fn finalize_only_starts_from_embeddings_stage() {
        let mut state = build_initial_state(build_init(false, false, false));
        assert!(start_finalize_if_ready(&mut state).is_none());
        assert_eq!(state.stage, SimilarityPrepStage::AwaitScan);

        let mut state = build_initial_state(build_init(true, false, false));
        let request = start_finalize_if_ready(&mut state).expect("finalize request");
        assert_eq!(state.stage, SimilarityPrepStage::Finalizing);
        assert_eq!(request.umap_version, "v1");
    }
}
