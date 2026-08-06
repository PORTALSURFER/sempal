use super::*;

#[test]
fn filter_section_projects_name_text_input() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel::from_folder_browser(&state, false);

    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    let input = frame
        .paint_plan
        .first_text_input()
        .expect("name filter should project a text input");

    assert_eq!(input.widget_id, NAME_FILTER_INPUT_ID);
    assert_eq!(input.state.value, "");
    assert_eq!(
        input.placeholder.as_ref().map(|value| value.as_str()),
        Some("ANY")
    );
    assert!(
        !frame
            .paint_plan
            .contains_text_after_x("ANY", input.rect.min.x),
        "name filter should not paint Any as a read-only property value"
    );
}

#[test]
fn filter_section_projects_tag_text_input_with_row_labels() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel::from_folder_browser(&state, false);

    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    let inputs = frame.paint_plan.text_inputs().collect::<Vec<_>>();

    assert!(frame.paint_plan.contains_text("NAME"));
    assert!(frame.paint_plan.contains_text("TAGS"));
    assert!(frame.paint_plan.contains_text("CURATE"));
    assert!(frame.paint_plan.contains_text("HARVEST"));
    assert!(frame.paint_plan.contains_text("TYPE"));
    assert!(frame.paint_plan.contains_text("RATING"));
    assert!(
        !frame.paint_plan.contains_text("Curat")
            && !frame.paint_plan.contains_text("Harve")
            && !frame.paint_plan.contains_text("Ratin")
    );
    assert_eq!(
        inputs
            .iter()
            .map(|input| input.widget_id)
            .collect::<Vec<_>>(),
        vec![NAME_FILTER_INPUT_ID, TAG_FILTER_INPUT_ID]
    );
}

#[test]
fn filter_section_filter_name_labels_are_readable_and_fit_label_cells() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel::from_folder_browser(&state, false);
    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    let layout = filter_section(&model)
        .view_layout_at_size(ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT));
    let labels = [
        ("NAME", "Name"),
        ("TAGS", "Tags"),
        ("CURATE", "Curate"),
        ("HARVEST", "Harvest"),
        ("TYPE", "Type"),
        ("RATING", "Rating"),
    ];
    let label_runs = labels
        .iter()
        .map(|(display_label, identity_label)| {
            frame
                .paint_plan
                .first_text_run(display_label)
                .map(|run| (run, *identity_label))
                .unwrap_or_else(|| panic!("missing filter label {display_label}"))
        })
        .collect::<Vec<_>>();

    assert!(
        label_runs
            .iter()
            .all(|(run, _)| run.font_size == label_runs[0].0.font_size)
    );
    for (run, identity_label) in label_runs {
        let widget_id = automation_filter_family_label_toggle_id(identity_label);
        let label_rect = layout
            .rects
            .get(&widget_id)
            .copied()
            .unwrap_or_else(|| panic!("missing {} label hit target", run.text));

        assert_eq!(label_rect.width(), FILTER_LABEL_WIDTH);
        assert!(
            run.rect.min.x >= label_rect.min.x && run.rect.max.x <= label_rect.max.x,
            "{} label text should fit inside the filter label cell, label={:?}, text={:?}",
            run.text,
            label_rect,
            run.rect
        );
        assert!(
            frame
                .paint_plan
                .fill_rects_for_widget(widget_id)
                .next()
                .is_none(),
            "{} label hit target must not paint a background",
            run.text
        );
        assert!(
            frame
                .paint_plan
                .stroke_rects_for_widget(widget_id)
                .next()
                .is_none(),
            "{} label hit target must not paint a border",
            run.text
        );
    }
}

#[test]
fn filter_section_rows_share_uniform_height_contract() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel::from_folder_browser(&state, false);
    let layout = filter_section(&model)
        .view_layout_at_size(ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT));
    let rows = [
        ("Name", NAME_FILTER_INPUT_ID),
        ("Tags", TAG_FILTER_INPUT_ID),
        ("Curate", CURATION_FILTER_DROPDOWN_TRIGGER_ID),
        ("Harvest", HARVEST_FILTER_DROPDOWN_TRIGGER_ID),
        ("Type", automation_playback_type_filter_toggle_id("1-Shot")),
        ("Rating", automation_rating_filter_toggle_id("T3")),
    ];
    let mut previous_row_top = None;

    for (label, control_id) in rows {
        let label_rect = layout
            .rects
            .get(&automation_filter_family_label_toggle_id(label))
            .copied()
            .unwrap_or_else(|| panic!("missing {label} label hit target"));
        let control_rect = layout
            .rects
            .get(&control_id)
            .copied()
            .unwrap_or_else(|| panic!("missing {label} control hit target"));
        let row_top = control_rect.min.y - FILTER_ROW_VERTICAL_INSET;

        assert_close(label_rect.height(), FILTER_ROW_CONTROL_HEIGHT);
        assert_close(control_rect.height(), FILTER_ROW_CONTROL_HEIGHT);
        assert_close(
            control_rect.min.y - label_rect.min.y,
            FILTER_LABEL_VERTICAL_SHIFT,
        );
        assert_close(
            control_rect.center().y - label_rect.center().y,
            FILTER_LABEL_VERTICAL_SHIFT,
        );
        assert_close(
            row_top + FILTER_ROW_HEIGHT,
            control_rect.max.y + FILTER_ROW_VERTICAL_INSET,
        );

        if let Some(previous_top) = previous_row_top {
            assert_close(
                row_top - previous_top,
                FILTER_ROW_HEIGHT + FILTER_ROW_SPACING,
            );
        }
        previous_row_top = Some(row_top);
    }
}

#[test]
fn filter_section_filter_labels_dispatch_family_enable_changes() {
    let mut state = FolderBrowserState::load_default();
    state.apply_message(FolderBrowserMessage::NameFilterInput(
        TextInputMessage::Changed {
            value: String::from("kick"),
        },
    ));
    state.set_rating_filter(1, true);
    let model = FilterSectionViewModel::from_folder_browser(&state, false);

    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_filter_family_label_toggle_id("Name"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetFilterFamilyEnabled(FilterFamily::Name, false)
        ))
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_filter_family_label_toggle_id("Rating"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetFilterFamilyEnabled(FilterFamily::Rating, false)
        ))
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_filter_family_label_toggle_id("Tags"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetFilterFamilyEnabled(FilterFamily::Tags, true)
        ))
    );
}

#[test]
fn enabled_filter_family_uses_shared_selected_row_chrome() {
    let inactive_state = FolderBrowserState::load_default();
    let inactive_model = FilterSectionViewModel::from_folder_browser(&inactive_state, false);
    let inactive_frame = filter_section(&inactive_model).view_frame_at_size_with_default_theme(
        ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT),
    );

    let mut active_state = FolderBrowserState::load_default();
    active_state.apply_message(FolderBrowserMessage::NameFilterInput(
        TextInputMessage::Changed {
            value: String::from("kick"),
        },
    ));
    let active_model = FilterSectionViewModel::from_folder_browser(&active_state, false);
    let active_frame = filter_section(&active_model).view_frame_at_size_with_default_theme(
        ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT),
    );
    let selected_fill = sidebar_row_selected_fill_for_tests();

    assert!(
        inactive_frame
            .paint_plan
            .fill_rects()
            .all(|fill| fill.color != selected_fill),
        "inactive filter rows must not paint persistent selection chrome"
    );
    assert!(
        active_frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == selected_fill),
        "enabled filters should use the shared selected-row fill"
    );
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.01,
        "expected {actual} to be within 0.01 of {expected}"
    );
}

#[test]
fn filter_section_projects_curation_scope_dropdown_and_dispatches_changes() {
    let mut state = FolderBrowserState::load_default();
    state.set_curation_scope(BrowserCurationScope::Ratings, true);
    let mut model = FilterSectionViewModel::from_folder_browser(&state, false);
    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    assert!(
        frame
            .paint_plan
            .first_widget_rect(CURATION_FILTER_DROPDOWN_TRIGGER_ID)
            .is_some()
    );
    assert!(frame.paint_plan.contains_text("RATE"));
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            CURATION_FILTER_DROPDOWN_TRIGGER_ID,
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::ToggleCurationFilterDropdown)
    );

    model.curation.dropdown_open = true;
    let open_frame = filter_section(&model).view_frame_at_size_with_default_theme(
        ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT),
    );
    assert!(
        open_frame
            .paint_plan
            .first_widget_rect(CURATION_FILTER_DROPDOWN_TRIGGER_ID)
            .is_some()
    );
    assert_eq!(
        open_frame
            .paint_plan
            .first_widget_rect(automation_curation_filter_dropdown_option_id("All")),
        None,
        "curation menu options should render in the sidebar overlay, not inside the clipped filter section"
    );

    let overlay_frame = curation_filter_dropdown_overlay(&model, 4.0, 167.0)
        .expect("open curation dropdown should project an overlay menu");
    let overlay_frame = overlay_frame.view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    let all_rect = overlay_frame
        .paint_plan
        .first_widget_rect(automation_curation_filter_dropdown_option_id("All"))
        .expect("All option should render in the overlay");
    let rate_rect = overlay_frame
        .paint_plan
        .first_widget_rect(automation_curation_filter_dropdown_option_id("Rate"))
        .expect("Rate option should render in the overlay");
    let tags_rect = overlay_frame
        .paint_plan
        .first_widget_rect(automation_curation_filter_dropdown_option_id("Tags"))
        .expect("Tags option should render in the overlay");
    assert!(all_rect.min.x > 0.0);
    assert!(
        (100.0..=160.0).contains(&all_rect.width()),
        "curation overlay options should stay fixed to the compact sidebar control width, got {}",
        all_rect.width()
    );
    assert!(all_rect.max.x <= 240.0);
    assert!(rate_rect.min.y > all_rect.min.y);
    assert!(tags_rect.min.y > rate_rect.min.y);
    assert_eq!(
        curation_filter_dropdown_overlay(&model, 4.0, 167.0)
            .expect("open curation dropdown should project an overlay menu")
            .view_dispatch_widget_output(
                automation_curation_filter_dropdown_option_id("Tags"),
                ui::WidgetOutput::typed(ButtonMessage::Activate {
                    provenance: ui::InteractionProvenance::Programmatic,
                }),
            ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::SetCurationScope(BrowserCurationScope::Tags, true)
        ))
    );
}

#[test]
fn filter_section_uses_harvest_dropdown_as_only_filter_mode_trigger() {
    let state = FolderBrowserState::load_default();
    let mut model = FilterSectionViewModel::from_folder_browser(&state, false);
    model.harvest.selected_filter = Some(HarvestFilter::NeedsReview);
    model.harvest.family_available = false;
    model.harvest.family_open = false;
    model.harvest.dropdown_open = false;
    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    let curation_rect = frame
        .paint_plan
        .first_widget_rect(CURATION_FILTER_DROPDOWN_TRIGGER_ID)
        .expect("Curation dropdown should render");
    let harvest_rect = frame
        .paint_plan
        .first_widget_rect(HARVEST_FILTER_DROPDOWN_TRIGGER_ID)
        .expect("Harvest dropdown should render");

    assert_eq!(
        frame
            .paint_plan
            .first_widget_rect(RETIRED_HARVEST_FILTER_ARROW_TOGGLE_ID),
        None,
        "Harvest should not render a separate arrow button for the dropdown mode"
    );
    assert!(frame.paint_plan.contains_text("NEEDS REVIEW"));
    assert_close(harvest_rect.min.x, curation_rect.min.x);
    assert_close(harvest_rect.width(), curation_rect.width());
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            HARVEST_FILTER_DROPDOWN_TRIGGER_ID,
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::ToggleHarvestFilterDropdown)
    );
}

#[test]
fn filter_section_projects_harvest_filter_dropdown_and_dispatches_changes() {
    let mut state = FolderBrowserState::load_default();
    state.set_harvest_filter(HarvestFilter::NeedsReview, true);
    let mut model = FilterSectionViewModel::from_folder_browser(&state, false);
    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    assert!(
        frame
            .paint_plan
            .first_widget_rect(HARVEST_FILTER_DROPDOWN_TRIGGER_ID)
            .is_some()
    );
    assert!(frame.paint_plan.contains_text("NEEDS REVIEW"));
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            HARVEST_FILTER_DROPDOWN_TRIGGER_ID,
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::ToggleHarvestFilterDropdown)
    );

    model.harvest.dropdown_open = true;
    let open_frame = filter_section(&model).view_frame_at_size_with_default_theme(
        ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT),
    );
    assert_eq!(
        open_frame
            .paint_plan
            .first_widget_rect(automation_harvest_filter_dropdown_option_id("Needs Review")),
        None,
        "harvest menu options should render in the sidebar overlay, not inside the clipped filter section"
    );

    let overlay_frame = harvest_filter_dropdown_overlay(&model, 4.0, 167.0)
        .expect("open harvest dropdown should project an overlay menu");
    let overlay_frame = overlay_frame.view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    for (label, filter) in [
        ("Needs Review", HarvestFilter::NeedsReview),
        ("Done", HarvestFilter::Done),
        ("Ignored", HarvestFilter::Ignored),
        ("All", HarvestFilter::All),
    ] {
        assert!(
            overlay_frame
                .paint_plan
                .first_widget_rect(automation_harvest_filter_dropdown_option_id(label))
                .is_some(),
            "{label} option should render in the overlay"
        );
        assert_eq!(
            harvest_filter_dropdown_overlay(&model, 4.0, 167.0)
                .expect("open harvest dropdown should project an overlay menu")
                .view_dispatch_widget_output(
                    automation_harvest_filter_dropdown_option_id(label),
                    ui::WidgetOutput::typed(ButtonMessage::Activate {
                        provenance: ui::InteractionProvenance::Programmatic,
                    }),
                ),
            Some(GuiMessage::FolderBrowser(
                FolderBrowserMessage::SetHarvestFilter(filter, true)
            ))
        );
    }
}

#[test]
fn filter_section_projects_default_harvest_all_after_family_toggle() {
    let mut state = FolderBrowserState::load_default();
    state.set_filter_family_enabled(FilterFamily::Harvest, true);
    let model = FilterSectionViewModel::from_folder_browser(&state, false);

    assert!(model.harvest.enabled);
    assert_eq!(model.harvest.selected_filter, Some(HarvestFilter::All));

    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    assert!(frame.paint_plan.contains_text("ALL"));
}

#[test]
fn filter_section_hides_clear_buttons_when_filters_are_empty() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel::from_folder_browser(&state, false);

    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    assert_eq!(
        frame
            .paint_plan
            .first_widget_rect(name_filter_clear_button_id()),
        None
    );
    assert_eq!(
        frame
            .paint_plan
            .first_widget_rect(tag_filter_clear_button_id()),
        None
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            name_filter_clear_button_id(),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        None
    );
}

#[test]
fn filter_section_projects_name_clear_button_for_active_name_filter() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel {
        name_filter: String::from("kick"),
        ..FilterSectionViewModel::from_folder_browser(&state, false)
    };

    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    assert!(
        frame
            .paint_plan
            .first_widget_rect(name_filter_clear_button_id())
            .is_some()
    );
    assert_eq!(
        frame
            .paint_plan
            .first_widget_rect(tag_filter_clear_button_id()),
        None
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            name_filter_clear_button_id(),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::NameFilterInput(empty_filter_message())
        ))
    );
}

#[test]
fn filter_section_projects_tag_clear_button_for_active_tag_filter() {
    let state = FolderBrowserState::load_default();
    let model = FilterSectionViewModel {
        tag_filter: String::from("drum"),
        ..FilterSectionViewModel::from_folder_browser(&state, false)
    };

    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));

    assert_eq!(
        frame
            .paint_plan
            .first_widget_rect(name_filter_clear_button_id()),
        None
    );
    assert!(
        frame
            .paint_plan
            .first_widget_rect(tag_filter_clear_button_id())
            .is_some()
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            tag_filter_clear_button_id(),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::TagFilterInput(empty_filter_message())
        ))
    );
}

#[test]
fn filter_section_projects_playback_type_toggles_and_dispatches_changes() {
    let mut state = FolderBrowserState::load_default();
    state.set_playback_type_filter(PlaybackTypeFilter::Loop, true);
    let model = FilterSectionViewModel::from_folder_browser(&state, false);
    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    let layout = filter_section(&model)
        .view_layout_at_size(ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT));

    assert!(
        layout
            .rects
            .get(&automation_playback_type_filter_toggle_id("1-Shot"))
            .is_some()
    );
    assert!(
        layout
            .rects
            .get(&automation_playback_type_filter_toggle_id("Loop"))
            .is_some()
    );
    assert!(frame.paint_plan.contains_text("1-SHOT,"));
    assert!(frame.paint_plan.contains_text("LOOP"));
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_playback_type_filter_toggle_id("1-Shot"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::TogglePlaybackTypeFilter(PlaybackTypeFilter::OneShot, true)
        ))
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_playback_type_filter_toggle_id("Loop"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::TogglePlaybackTypeFilter(PlaybackTypeFilter::Loop, false)
        ))
    );
}

#[test]
fn filter_section_playback_type_toggles_share_style_family() {
    let inactive_frame = playback_type_filter_frame(&[]);
    let inactive_one_shot = playback_type_toggle_text_color(&inactive_frame.paint_plan, "1-Shot");
    let inactive_loop = playback_type_toggle_text_color(&inactive_frame.paint_plan, "Loop");
    assert_eq!(
        inactive_one_shot, inactive_loop,
        "inactive playback-type toggles should render with matching text colors"
    );

    let active_frame =
        playback_type_filter_frame(&[PlaybackTypeFilter::OneShot, PlaybackTypeFilter::Loop]);
    let active_one_shot = playback_type_toggle_text_color(&active_frame.paint_plan, "1-Shot");
    let active_loop = playback_type_toggle_text_color(&active_frame.paint_plan, "Loop");
    assert_eq!(
        active_one_shot, active_loop,
        "active playback-type toggles should render with matching text colors"
    );
    assert_ne!(
        inactive_one_shot, active_one_shot,
        "active and inactive playback-type toggle states should remain visually distinct"
    );
    for label in ["1-Shot", "Loop"] {
        let widget_id = automation_playback_type_filter_toggle_id(label);
        assert!(
            inactive_frame
                .paint_plan
                .fill_rects_for_widget(widget_id)
                .next()
                .is_none()
        );
        assert!(
            inactive_frame
                .paint_plan
                .stroke_rects_for_widget(widget_id)
                .next()
                .is_none()
        );
        assert!(
            active_frame
                .paint_plan
                .fill_rects_for_widget(widget_id)
                .next()
                .is_none()
        );
        assert!(
            active_frame
                .paint_plan
                .stroke_rects_for_widget(widget_id)
                .next()
                .is_none()
        );
    }
}

fn playback_type_filter_frame(active_filters: &[PlaybackTypeFilter]) -> SurfaceFrame {
    let mut state = FolderBrowserState::load_default();
    for filter in active_filters {
        state.set_playback_type_filter(*filter, true);
    }
    let model = FilterSectionViewModel::from_folder_browser(&state, false);
    filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ))
}

fn playback_type_toggle_text_color(plan: &SurfacePaintPlan, label: &'static str) -> Rgba8 {
    let display_label = if label == "1-Shot" { "1-SHOT," } else { "LOOP" };
    plan.first_text_color(display_label)
        .unwrap_or_else(|| panic!("missing label for {label} playback-type toggle"))
}

#[test]
fn filter_section_projects_rating_toggles_and_dispatches_changes() {
    let mut state = FolderBrowserState::load_default();
    state.set_rating_filter(-3, true);
    state.set_rating_filter(0, true);
    let model = FilterSectionViewModel::from_folder_browser(&state, false);
    let frame = filter_section(&model).view_frame_at_size_with_default_theme(ui::Vector2::new(
        240.0,
        FILTER_SECTION_TEST_FRAME_HEIGHT,
    ));
    let layout = filter_section(&model)
        .view_layout_at_size(ui::Vector2::new(240.0, FILTER_SECTION_TEST_FRAME_HEIGHT));

    assert!(
        layout
            .rects
            .get(&automation_rating_filter_toggle_id("T3"))
            .is_some()
    );
    assert!(
        layout
            .rects
            .get(&automation_rating_filter_toggle_id("U"))
            .is_some()
    );
    assert!(
        layout
            .rects
            .get(&automation_rating_filter_toggle_id("K4"))
            .is_some()
    );
    assert!(frame.paint_plan.fill_rects().any(|fill| {
        fill.color == rating_filter_swatch_color(-3, true)
            && fill.rect.width() == RATING_FILTER_SWATCH_SIZE as f32
    }));
    assert!(
        frame
            .paint_plan
            .fill_rects()
            .any(|fill| fill.color == rating_filter_swatch_color(1, false))
    );
    for label in ["T3", "U", "K4"] {
        let widget_id = automation_rating_filter_toggle_id(label);
        assert!(
            frame
                .paint_plan
                .fill_rects_for_widget(widget_id)
                .next()
                .is_none(),
            "rating swatch input {label} must not paint a button surface"
        );
        assert!(
            frame
                .paint_plan
                .stroke_rects_for_widget(widget_id)
                .next()
                .is_none(),
            "rating swatch input {label} must not paint a border"
        );
    }
    assert!(
        !frame
            .paint_plan
            .text_labels()
            .any(|label| matches!(label, "T3" | "T2" | "T1" | "U" | "K1" | "K2" | "K3" | "K4"))
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_rating_filter_toggle_id("K4"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ToggleRatingFilter(4, true)
        ))
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_rating_filter_toggle_id("U"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ToggleRatingFilter(0, false)
        ))
    );
    assert_eq!(
        filter_section(&model).view_dispatch_widget_output(
            automation_rating_filter_toggle_id("T3"),
            ui::WidgetOutput::typed(ButtonMessage::Activate {
                provenance: ui::InteractionProvenance::Programmatic,
            }),
        ),
        Some(GuiMessage::FolderBrowser(
            FolderBrowserMessage::ToggleRatingFilter(-3, false)
        ))
    );
}

#[test]
fn inactive_rating_filter_swatches_are_toned_down_without_changing_active_colors() {
    for (level, expected_active) in [
        (
            -3,
            Rgba8 {
                r: 238,
                g: 77,
                b: 67,
                a: 235,
            },
        ),
        (
            0,
            Rgba8 {
                r: 184,
                g: 188,
                b: 194,
                a: 235,
            },
        ),
        (
            4,
            Rgba8 {
                r: 232,
                g: 188,
                b: 56,
                a: 245,
            },
        ),
    ] {
        let active = rating_filter_swatch_color(level, true);
        let inactive = rating_filter_swatch_color(level, false);

        assert_eq!(active, expected_active);
        if level == 0 {
            assert_eq!(
                inactive,
                Rgba8 {
                    r: 138,
                    g: 142,
                    b: 148,
                    a: 92,
                }
            );
            assert!(inactive.r < active.r && inactive.g < active.g && inactive.b < active.b);
        } else {
            assert_eq!(
                (inactive.r, inactive.g, inactive.b),
                (active.r, active.g, active.b)
            );
        }
        assert!(
            inactive.a <= active.a.saturating_sub(120),
            "inactive rating swatch for {level} should be materially dimmer"
        );
    }
}
