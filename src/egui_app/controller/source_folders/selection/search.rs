use super::super::*;

impl EguiController {
    pub(crate) fn set_folder_search(&mut self, query: String) {
        if self.selection_state.ctx.selected_source.is_none() {
            self.ui.sources.folders.search_query = query;
            return;
        }
        let snapshot = {
            let Some(model) = self.current_folder_model_mut() else {
                self.ui.sources.folders.search_query = query;
                return;
            };
            if model.search_query == query {
                return;
            }
            model.search_query = query.clone();
            model.clone()
        };
        self.ui.sources.folders.search_query = query;
        self.build_folder_rows(&snapshot);
    }

    pub(crate) fn focus_folder_search(&mut self) {
        self.ui.sources.folders.search_focus_requested = true;
        self.focus_folder_context();
    }
}
