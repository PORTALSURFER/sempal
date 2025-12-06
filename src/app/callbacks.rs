use super::*;

/// Wire UI callbacks to the shared handler instance.
pub(super) fn attach_callbacks(app: &Sempal, drop_handler: &DropHandler) {
    let seek_handler = drop_handler.clone();
    app.on_seek_requested(move |position| seek_handler.seek_to(position));
    let selection_start_handler = drop_handler.clone();
    app.on_selection_drag_started(move |position| {
        selection_start_handler.start_selection_drag(position)
    });
    let selection_update_handler = drop_handler.clone();
    app.on_selection_drag_updated(move |position| {
        selection_update_handler.update_selection_drag(position)
    });
    let selection_finish_handler = drop_handler.clone();
    app.on_selection_drag_finished(move || selection_finish_handler.finish_selection_drag());
    let selection_clear_handler = drop_handler.clone();
    app.on_selection_clear_requested(move || selection_clear_handler.clear_selection_request());
    let selection_handle_handler = drop_handler.clone();
    app.on_selection_handle_pressed(move |is_start| {
        let edge = if is_start {
            SelectionEdge::Start
        } else {
            SelectionEdge::End
        };
        selection_handle_handler.begin_edge_drag(edge);
    });
    let loop_handler = drop_handler.clone();
    app.on_loop_toggled(move |enabled| loop_handler.handle_loop_toggle(enabled));
    let add_handler = drop_handler.clone();
    app.on_add_source(move || add_handler.handle_add_source());
    let source_handler = drop_handler.clone();
    app.on_source_selected(move |index| source_handler.handle_source_selected(index));
    let update_handler = drop_handler.clone();
    app.on_source_update_requested(move |index| update_handler.handle_update_source(index));
    let remove_handler = drop_handler.clone();
    app.on_source_remove_requested(move |index| remove_handler.handle_remove_source(index));
    let wav_handler = drop_handler.clone();
    app.on_wav_clicked(move |path| wav_handler.handle_wav_clicked(path));
    let collection_add_handler = drop_handler.clone();
    app.on_add_collection(move || collection_add_handler.handle_add_collection());
    let collection_select_handler = drop_handler.clone();
    app.on_collection_selected(move |index| {
        collection_select_handler.handle_collection_selected(index)
    });
    let drop_handler_clone = drop_handler.clone();
    app.on_sample_dropped_on_collection(move |collection_id, path| {
        drop_handler_clone.handle_sample_dropped_on_collection(collection_id, path)
    });
    let close_handler = drop_handler.clone();
    app.on_close_requested(move || {
        close_handler.shutdown();
        let _ = slint::quit_event_loop();
    });
}
