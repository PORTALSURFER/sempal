use super::*;

pub(super) fn handle_similarity_prepared(
    controller: &mut EguiController,
    message: jobs::SimilarityPrepResult,
) {
    controller.handle_similarity_prep_result(message);
}
