use super::*;
use crate::egui_app::state::{SampleBrowserSort, SimilarQuery};

pub(crate) fn apply_similarity_query(controller: &mut EguiController, query: SimilarQuery) {
    controller.ui.browser.similar_query = Some(query);
    controller.ui.browser.sort = SampleBrowserSort::Similarity;
    controller.ui.browser.similarity_sort_follow_loaded = false;
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
}

pub(crate) fn clear_similar_filter(controller: &mut EguiController) {
    if controller.ui.browser.similar_query.take().is_some() {
        controller.ui.browser.sort = SampleBrowserSort::ListOrder;
        controller.ui.browser.similarity_sort_follow_loaded = false;
        controller.rebuild_browser_lists();
    }
}

pub(crate) fn disable_similarity_sort(controller: &mut EguiController) {
    controller.ui.browser.sort = SampleBrowserSort::ListOrder;
    controller.ui.browser.similarity_sort_follow_loaded = false;
    controller.ui.browser.similar_query = None;
    controller.rebuild_browser_lists();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support::dummy_controller;

    #[test]
    fn apply_similarity_query_resets_browser_state_and_preserves_anchor() {
        let (mut controller, _source) = dummy_controller();
        controller.ui.browser.search_query = "query".to_string();
        controller.ui.browser.search_focus_requested = true;
        controller.ui.browser.sort = SampleBrowserSort::ListOrder;
        controller.ui.browser.similarity_sort_follow_loaded = true;
        let query = SimilarQuery {
            sample_id: "sample-id".to_string(),
            label: "Sample".to_string(),
            indices: vec![0],
            scores: vec![0.5],
            anchor_index: Some(2),
        };
        apply_similarity_query(&mut controller, query);
        let applied = controller.ui.browser.similar_query.as_ref().unwrap();
        assert_eq!(controller.ui.browser.sort, SampleBrowserSort::Similarity);
        assert!(!controller.ui.browser.similarity_sort_follow_loaded);
        assert!(controller.ui.browser.search_query.is_empty());
        assert!(!controller.ui.browser.search_focus_requested);
        assert_eq!(applied.anchor_index, Some(2));
    }
}
