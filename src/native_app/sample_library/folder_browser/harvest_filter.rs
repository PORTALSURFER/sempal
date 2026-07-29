use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use wavecrate::sample_sources::{HarvestState, SourceId};

use super::{FileEntry, FolderBrowserState};

pub(in crate::native_app) const DEFAULT_HARVEST_FILTER: HarvestFilter = HarvestFilter::All;

pub(in crate::native_app) const HARVEST_FILTERS: [HarvestFilter; 9] = [
    HarvestFilter::New,
    HarvestFilter::NewAndTouched,
    HarvestFilter::NeedsReview,
    HarvestFilter::Touched,
    HarvestFilter::HasDerivatives,
    HarvestFilter::NoDerivatives,
    HarvestFilter::Done,
    HarvestFilter::Ignored,
    HarvestFilter::All,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum HarvestFilter {
    New,
    NewAndTouched,
    NeedsReview,
    Touched,
    HasDerivatives,
    NoDerivatives,
    Done,
    Ignored,
    All,
}

impl FolderBrowserState {
    pub(in crate::native_app::sample_library::folder_browser) fn retain_harvest_filter_matches(
        &self,
        files: &mut Vec<&FileEntry>,
        reveal_id: Option<&str>,
    ) {
        let Some(filter) = self.active_harvest_filter() else {
            return;
        };
        if filter == HarvestFilter::All {
            return;
        }
        let lookup = HarvestFileFactsLookup::load(self, files);
        files.retain(|file| {
            reveal_id == Some(file.id.as_str()) || lookup.file_matches(self, file, filter)
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app::sample_library::folder_browser) struct HarvestFileFacts {
    pub(in crate::native_app::sample_library::folder_browser) state: HarvestState,
    pub(in crate::native_app::sample_library::folder_browser) derivative_count: u64,
}

#[derive(Debug, Default)]
pub(in crate::native_app::sample_library::folder_browser) struct HarvestFileFactsLookup {
    states: HashMap<(String, PathBuf), HarvestState>,
    derivative_counts: HashMap<(String, PathBuf), u64>,
}

impl HarvestFileFactsLookup {
    pub(in crate::native_app::sample_library::folder_browser) fn load(
        browser: &FolderBrowserState,
        files: &[&FileEntry],
    ) -> Self {
        let source_ids = files
            .iter()
            .filter_map(|file| browser.source_id_for_file_path(Path::new(&file.id)))
            .collect::<HashSet<_>>();
        let mut lookup = Self::default();
        for source_id in source_ids {
            let source = SourceId::from_string(source_id.clone());
            match wavecrate::sample_sources::library::harvest_files_for_source(&source) {
                Ok(records) => {
                    for record in records {
                        lookup
                            .states
                            .insert((source_id.clone(), record.key.relative_path), record.state);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        source_id,
                        "failed to load harvest file filter state: {error}"
                    );
                }
            }
            match wavecrate::sample_sources::library::harvest_derivative_counts_for_source(&source)
            {
                Ok(counts) => {
                    for (relative_path, count) in counts {
                        lookup
                            .derivative_counts
                            .insert((source_id.clone(), relative_path), count);
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        source_id,
                        "failed to load harvest derivative filter state: {error}"
                    );
                }
            }
        }
        lookup
    }

    pub(in crate::native_app::sample_library::folder_browser) fn facts_for_file(
        &self,
        browser: &FolderBrowserState,
        file: &FileEntry,
    ) -> Option<HarvestFileFacts> {
        let (source_id, relative_path) = browser.harvest_filter_key_for_file(&file.id)?;
        let state_key = (source_id.clone(), relative_path.clone());
        let state = self.states.get(&state_key).copied();
        let derivative_count = self
            .derivative_counts
            .get(&(source_id, relative_path))
            .copied()
            .unwrap_or(0);
        Some(HarvestFileFacts {
            state: state.unwrap_or(HarvestState::New),
            derivative_count,
        })
    }

    fn file_matches(
        &self,
        browser: &FolderBrowserState,
        file: &FileEntry,
        filter: HarvestFilter,
    ) -> bool {
        self.facts_for_file(browser, file).is_some_and(|facts| {
            harvest_state_matches_filter(facts.state, facts.derivative_count, filter)
        })
    }
}

pub(in crate::native_app::sample_library::folder_browser) fn harvest_badges_for_facts(
    facts: Option<HarvestFileFacts>,
    show_new_untracked: bool,
) -> Vec<String> {
    let Some(facts) = facts else {
        return Vec::new();
    };
    let show_state_badge = facts.state != HarvestState::New || show_new_untracked;
    let mut badges = Vec::with_capacity(2);
    if show_state_badge {
        badges.push(harvest_state_badge(facts.state).to_owned());
    }
    if facts.derivative_count > 0 {
        badges.push(format!("D{}", facts.derivative_count.min(99)));
    }
    badges
}

fn harvest_state_badge(state: HarvestState) -> &'static str {
    match state {
        HarvestState::New => "new",
        HarvestState::Seen => "seen",
        HarvestState::Touched => "touch",
        HarvestState::Done => "done",
        HarvestState::Ignored => "ign",
    }
}

fn harvest_state_matches_filter(
    state: HarvestState,
    derivative_count: u64,
    filter: HarvestFilter,
) -> bool {
    match filter {
        HarvestFilter::New => state == HarvestState::New || state == HarvestState::Seen,
        HarvestFilter::NewAndTouched => {
            matches!(
                state,
                HarvestState::New | HarvestState::Seen | HarvestState::Touched
            )
        }
        HarvestFilter::NeedsReview => {
            !matches!(state, HarvestState::Done | HarvestState::Ignored) && derivative_count == 0
        }
        HarvestFilter::Touched => state == HarvestState::Touched,
        HarvestFilter::HasDerivatives => derivative_count > 0,
        HarvestFilter::NoDerivatives => derivative_count == 0,
        HarvestFilter::Done => state == HarvestState::Done,
        HarvestFilter::Ignored => state == HarvestState::Ignored,
        HarvestFilter::All => true,
    }
}

impl FolderBrowserState {
    pub(in crate::native_app) fn source_id_for_file_path(
        &self,
        file_path: &Path,
    ) -> Option<String> {
        self.source
            .sources
            .iter()
            .filter(|source| file_path.starts_with(&source.root))
            .max_by_key(|source| source.root.components().count())
            .map(|source| source.id.clone())
    }

    fn harvest_filter_key_for_file(&self, file_id: &str) -> Option<(String, PathBuf)> {
        let file_path = Path::new(file_id);
        self.source
            .sources
            .iter()
            .filter_map(|source| {
                file_path.strip_prefix(&source.root).ok().map(|relative| {
                    (
                        source.id.clone(),
                        relative.to_path_buf(),
                        source.root.components().count(),
                    )
                })
            })
            .max_by_key(|(_, _, root_depth)| *root_depth)
            .map(|(source_id, relative_path, _)| (source_id, relative_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harvest_filter_matching_keeps_manual_done_and_ignored_out_of_review_queue() {
        assert!(harvest_state_matches_filter(
            HarvestState::New,
            0,
            HarvestFilter::NeedsReview
        ));
        assert!(harvest_state_matches_filter(
            HarvestState::Touched,
            0,
            HarvestFilter::NeedsReview
        ));
        assert!(!harvest_state_matches_filter(
            HarvestState::Done,
            0,
            HarvestFilter::NeedsReview
        ));
        assert!(!harvest_state_matches_filter(
            HarvestState::Ignored,
            0,
            HarvestFilter::NeedsReview
        ));
        assert!(!harvest_state_matches_filter(
            HarvestState::Touched,
            1,
            HarvestFilter::NeedsReview
        ));
    }

    #[test]
    fn harvest_filter_matching_distinguishes_derivative_queues() {
        assert!(harvest_state_matches_filter(
            HarvestState::Touched,
            2,
            HarvestFilter::HasDerivatives
        ));
        assert!(!harvest_state_matches_filter(
            HarvestState::Touched,
            0,
            HarvestFilter::HasDerivatives
        ));
        assert!(harvest_state_matches_filter(
            HarvestState::Done,
            0,
            HarvestFilter::NoDerivatives
        ));
        assert!(!harvest_state_matches_filter(
            HarvestState::New,
            1,
            HarvestFilter::NoDerivatives
        ));
    }

    #[test]
    fn harvest_filter_matching_supports_combined_new_and_touched_queue() {
        for state in [HarvestState::New, HarvestState::Seen, HarvestState::Touched] {
            assert!(harvest_state_matches_filter(
                state,
                0,
                HarvestFilter::NewAndTouched
            ));
        }
        assert!(!harvest_state_matches_filter(
            HarvestState::Done,
            0,
            HarvestFilter::NewAndTouched
        ));
    }

    #[test]
    fn harvest_badges_show_state_and_derivative_count() {
        assert_eq!(
            harvest_badges_for_facts(
                Some(HarvestFileFacts {
                    state: HarvestState::Touched,
                    derivative_count: 3,
                }),
                false,
            ),
            vec![String::from("touch"), String::from("D3")]
        );
        assert_eq!(
            harvest_badges_for_facts(
                Some(HarvestFileFacts {
                    state: HarvestState::Done,
                    derivative_count: 0,
                }),
                false,
            ),
            vec![String::from("done")]
        );
    }

    #[test]
    fn harvest_badges_keep_new_rows_quiet_outside_harvest_mode() {
        let facts = Some(HarvestFileFacts {
            state: HarvestState::New,
            derivative_count: 0,
        });

        assert!(harvest_badges_for_facts(facts, false).is_empty());
        assert_eq!(
            harvest_badges_for_facts(facts, true),
            vec![String::from("new")]
        );
        assert!(
            harvest_badges_for_facts(
                Some(HarvestFileFacts {
                    state: HarvestState::New,
                    derivative_count: 0,
                }),
                false,
            )
            .is_empty()
        );
    }
}
