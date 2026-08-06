use super::*;

#[test]
fn metadata_autocomplete_suffix_is_not_editable_input_text() {
    let (mut state, _source_root, _selected_file) =
        native_app_state_with_temp_sample("tag-target.wav");
    state
        .metadata
        .tags_by_file
        .insert(String::from("known.wav"), vec![String::from("kick")]);
    state.metadata.tag_draft = String::from("ki");

    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame_with_default_theme();
    let input_id = metadata_tag_text_input(&frame)
        .map(|input| input.widget_id)
        .expect("metadata tag input should paint");
    assert!(runtime.focus_widget(input_id));

    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::key_press(WidgetKey::Backspace)),
        Some(input_id)
    );
    assert_eq!(runtime.bridge().state().metadata.tag_draft, "k");
    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::key_press(WidgetKey::Backspace)),
        Some(input_id)
    );
    assert!(runtime.bridge().state().metadata.tag_draft.is_empty());
    assert!(!runtime.bridge().state().metadata_tag_completion_active());

    let frame = runtime.frame_with_default_theme();
    let tag_input = frame
        .paint_plan
        .text_inputs()
        .find(|input| input.widget_id == input_id)
        .expect("metadata tag input should still paint");
    assert!(tag_input.state.value.is_empty());
    assert_eq!(tag_input.state.caret, 0);
    assert_eq!(tag_input.state.selection_anchor, 0);
    assert!(!frame.paint_plan.contains_text("ick"));
}

#[test]
fn metadata_autocomplete_enter_commits_typed_prefix_without_selecting_first_suggestion() {
    let (mut state, _source_root, selected_file) =
        native_app_state_with_temp_sample("tag-target.wav");
    state
        .metadata
        .tags_by_file
        .insert(String::from("known.wav"), vec![String::from("kick")]);

    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame_with_default_theme();
    let input_id = metadata_tag_text_input(&frame)
        .map(|input| input.widget_id)
        .expect("metadata tag input should paint");
    assert!(runtime.focus_widget(input_id));

    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::character('k')),
        Some(input_id)
    );
    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::character('i')),
        Some(input_id)
    );

    let state = runtime.bridge().state();
    assert_eq!(state.metadata.tag_draft, "ki");
    assert_eq!(
        state.metadata_tag_completion_suffix().as_deref(),
        Some("ck")
    );
    assert_eq!(
        state
            .metadata_tag_completion_options()
            .iter()
            .find(|option| option.selected)
            .map(|option| option.tag.as_str()),
        None
    );

    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::key_press(WidgetKey::Enter)),
        Some(input_id)
    );

    let state = runtime.bridge().state();
    assert_eq!(state.metadata.tags_by_file.get(&selected_file), None);
    assert_eq!(state.pending_metadata_tag_category_tag(), Some("ki"));
    assert_eq!(state.ui.status.sample, "Choose a category for ki");
}
