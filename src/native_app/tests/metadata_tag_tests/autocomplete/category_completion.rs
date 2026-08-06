use super::*;

#[test]
fn metadata_category_completion_mouse_click_commits_pending_tag() {
    let (state, _source_root, selected_file) = native_app_state_with_temp_sample("tag-target.wav");

    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame_with_default_theme();
    let input_id = metadata_tag_text_input(&frame)
        .map(|input| input.widget_id)
        .expect("metadata tag input should paint");
    assert!(runtime.focus_widget(input_id));

    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::character('d')),
        Some(input_id)
    );
    for character in "eep-kick".chars() {
        runtime.dispatch_focused_input(WidgetInput::character(character));
    }
    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::key_press(WidgetKey::Enter)),
        Some(input_id)
    );

    let category_rect = runtime
        .frame_with_default_theme()
        .paint_plan
        .first_text_rect("Character")
        .expect("category option should paint");
    let click = runtime.dispatch_primary_click(category_rect.center());
    assert!(
        click.completed_on_same_widget(),
        "category click should retain one target across press/release, click={click:?}"
    );

    let state = runtime.bridge().state();
    assert_eq!(
        state.metadata.tags_by_file.get(&selected_file),
        Some(&vec![String::from("deep-kick")])
    );
    assert_eq!(
        state
            .metadata
            .tag_dictionary
            .get("deep-kick")
            .map(String::as_str),
        Some("character")
    );
    assert_eq!(state.pending_metadata_tag_category_tag(), None);
}

#[test]
fn metadata_category_completion_pointer_hover_updates_active_category() {
    let (state, _source_root, _selected_file) = native_app_state_with_temp_sample("tag-target.wav");

    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame_with_default_theme();
    let input_id = metadata_tag_text_input(&frame)
        .map(|input| input.widget_id)
        .expect("metadata tag input should paint");
    assert!(runtime.focus_widget(input_id));

    for character in "deep-kick".chars() {
        runtime.dispatch_focused_input(WidgetInput::character(character));
    }
    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::key_press(WidgetKey::Enter)),
        Some(input_id)
    );

    let category_rect = runtime
        .frame_with_default_theme()
        .paint_plan
        .first_text_rect("Character")
        .expect("category option should paint");
    runtime.dispatch_input_at(
        category_rect.center(),
        WidgetInput::PointerMove {
            position: category_rect.center(),
            modifiers: Default::default(),
            timestamp: None,
            sequence_range: None,
        },
    );

    assert_eq!(
        runtime
            .bridge()
            .state()
            .metadata_tag_completion_options()
            .iter()
            .find(|option| option.selected)
            .map(|option| option.tag.as_str()),
        Some("Character")
    );
}

#[test]
fn metadata_category_completion_pointer_hover_uses_full_row_width() {
    let (state, _source_root, _selected_file) = native_app_state_with_temp_sample("tag-target.wav");

    let mut runtime = native_runtime_for_tests(state, Vector2::new(900.0, 620.0));
    let frame = runtime.frame_with_default_theme();
    let input_id = metadata_tag_text_input(&frame)
        .map(|input| input.widget_id)
        .expect("metadata tag input should paint");
    assert!(runtime.focus_widget(input_id));

    for character in "deep-kick".chars() {
        runtime.dispatch_focused_input(WidgetInput::character(character));
    }
    assert_eq!(
        runtime.dispatch_focused_input(WidgetInput::key_press(WidgetKey::Enter)),
        Some(input_id)
    );

    let category_rect = runtime
        .frame_with_default_theme()
        .paint_plan
        .first_text_rect("Character")
        .expect("category option should paint");
    let right_side = Point::new(category_rect.max.x + 120.0, category_rect.center().y);
    runtime.dispatch_input_at(
        right_side,
        WidgetInput::PointerMove {
            position: right_side,
            modifiers: Default::default(),
            timestamp: None,
            sequence_range: None,
        },
    );

    assert_eq!(
        runtime
            .bridge()
            .state()
            .metadata_tag_completion_options()
            .iter()
            .find(|option| option.selected)
            .map(|option| option.tag.as_str()),
        Some("Character")
    );
}
