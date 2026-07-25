use super::*;
use std::path::Path;
use wavecrate::sample_sources::SampleSource;
use wavecrate_analysis::aspects::SimilarityAspect;

fn test_starmap_item(file_id: &str, x: f32, y: f32) -> StarmapItem {
    StarmapItem {
        file_id: file_id.to_string(),
        label: file_id.to_string(),
        x,
        y,
        color: ui::Rgba8::new(57, 187, 245, 220),
        selected: false,
        focused: false,
        selection_flash: false,
        copy_flash: false,
        similarity_anchor: false,
        instant_audition_ready: true,
        preview_audition_ready: false,
        preview_audition_candidate: true,
        missing: false,
    }
}

#[test]
fn starmap_projection_index_returns_nearby_preview_warm_candidates_without_full_scan() {
    let mut items = Vec::new();
    for index in 0..2_000 {
        items.push(test_starmap_item(
            &format!("far-{index:04}.wav"),
            0.94,
            0.94,
        ));
    }
    for index in 0..96 {
        let offset = (index as f32 % 12.0) * 0.0007;
        items.push(test_starmap_item(
            &format!("near-{index:03}.wav"),
            0.498 + offset,
            0.502 + offset,
        ));
    }
    let items = Arc::<[StarmapItem]>::from(items);
    let index = StarmapProjectionIndex::build(&items);

    let scan = index.preview_warm_indices(&items, 0.5, 0.5, 32.0, 0.08, None, 24);

    assert_eq!(scan.indices.len(), 24);
    assert!(
        scan.inspected_count < 128,
        "zoomed dense warm planning should visit nearby cells, not all {} items",
        items.len()
    );
    assert!(
        scan.indices
            .iter()
            .all(|&item_index| items[item_index].file_id.starts_with("near-"))
    );
    assert!(
        scan.visited_cell_count <= scan.cell_count,
        "visited cells should be bounded by occupied viewport cells"
    );
    assert!(
        scan.cell_count < STARMAP_PROJECTION_INDEX_GRID as usize,
        "zoomed warm planning should not sort the whole projection grid"
    );
}

#[test]
fn starmap_projection_index_skips_empty_preview_warm_cells() {
    let items = Arc::<[StarmapItem]>::from(vec![
        test_starmap_item("center.wav", 0.50, 0.50),
        test_starmap_item("top-left.wav", 0.05, 0.05),
        test_starmap_item("bottom-right.wav", 0.95, 0.95),
    ]);
    let index = StarmapProjectionIndex::build(&items);

    let scan = index.preview_warm_indices(&items, 0.5, 0.5, 1.0, 0.0, None, 1);

    assert_eq!(scan.indices.len(), 1);
    assert_eq!(
        scan.cell_count,
        index.occupied_cell_count(),
        "full-map warm planning should sort occupied cells, not every empty grid cell"
    );
    assert_eq!(
        scan.visited_cell_count, 1,
        "warm planning should stop walking cells once the requested candidates are found"
    );
}

#[test]
fn starmap_projection_index_keeps_selected_preview_warm_candidate_first() {
    let items = Arc::<[StarmapItem]>::from(vec![
        test_starmap_item("selected.wav", 0.95, 0.95),
        test_starmap_item("near.wav", 0.5, 0.5),
    ]);
    let index = StarmapProjectionIndex::build(&items);

    let scan = index.preview_warm_indices(&items, 0.5, 0.5, 32.0, 0.08, Some("selected.wav"), 2);

    assert_eq!(scan.indices, vec![0, 1]);
    assert_eq!(scan.inspected_count, 2);
}

fn write_sparse_wav_i16(path: &Path, channels: u16, frames: u32) {
    let channels = channels.max(1);
    let sample_rate = 48_000_u32;
    let bits_per_sample = 16_u16;
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_bytes = frames
        .checked_mul(u32::from(block_align))
        .expect("test wav data size");
    let riff_size = 36_u32.checked_add(data_bytes).expect("test wav riff size");
    let mut file = std::fs::File::create(path).expect("create sparse wav");
    use std::io::Write;
    file.write_all(b"RIFF").expect("write riff");
    file.write_all(&riff_size.to_le_bytes())
        .expect("write riff size");
    file.write_all(b"WAVE").expect("write wave");
    file.write_all(b"fmt ").expect("write fmt");
    file.write_all(&16_u32.to_le_bytes())
        .expect("write fmt size");
    file.write_all(&1_u16.to_le_bytes())
        .expect("write pcm format");
    file.write_all(&channels.to_le_bytes())
        .expect("write channels");
    file.write_all(&sample_rate.to_le_bytes())
        .expect("write sample rate");
    file.write_all(&byte_rate.to_le_bytes())
        .expect("write byte rate");
    file.write_all(&block_align.to_le_bytes())
        .expect("write block align");
    file.write_all(&bits_per_sample.to_le_bytes())
        .expect("write bits");
    file.write_all(b"data").expect("write data chunk");
    file.write_all(&data_bytes.to_le_bytes())
        .expect("write data size");
    file.set_len(44_u64 + u64::from(data_bytes))
        .expect("extend sparse wav");
}

fn test_layout_point(index: usize, total: usize) -> wavecrate::sample_sources::StarmapLayoutPoint {
    let total = total.max(1) as f32;
    let t = (index as f32 + 0.5) / total;
    wavecrate::sample_sources::StarmapLayoutPoint {
        x: 0.10 + 0.80 * t,
        y: 0.50 + ((index % 5) as f32 - 2.0) * 0.04,
        cluster_id: None,
    }
}

fn complete_test_starmap_layout(
    browser: &mut FolderBrowserState,
    tags_by_file: &HashMap<String, Vec<String>>,
    file_ids: &[String],
) {
    browser.prepare_starmap_layout(tags_by_file);
    let request = browser
        .take_starmap_layout_load_request(tags_by_file)
        .expect("starmap layout request");
    browser.apply_starmap_layout_load_result(StarmapLayoutLoadResult {
        signature: request.signature,
        result: Ok(file_ids
            .iter()
            .enumerate()
            .map(|(index, file_id)| (file_id.clone(), test_layout_point(index, file_ids.len())))
            .collect()),
    });
}

#[test]
fn starmap_position_uses_normalized_layout_when_available() {
    let position = starmap_position(StarmapLayoutPoint {
        x: 0.25,
        y: 0.75,
        cluster_id: None,
    });

    assert_eq!(position, (0.25, 0.75));
}

#[test]
fn starmap_projection_omits_missing_layout_rows_before_and_after_load_completes() {
    let root = tempfile::tempdir().expect("source root");
    let positioned = root.path().join("positioned.wav");
    let missing = root.path().join("missing.wav");
    std::fs::write(&positioned, []).expect("write positioned");
    std::fs::write(&missing, []).expect("write missing");
    let positioned_id = positioned.to_string_lossy().to_string();
    let missing_id = missing.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    let request = browser
        .take_starmap_layout_load_request(&tags_by_file)
        .expect("layout request");
    assert!(
        browser
            .starmap_projection(StarmapProjection {
                tags_by_file: &tags_by_file,
                instant_audition_sample_paths: &HashSet::new(),
                preview_audition_sample_paths: &HashSet::new(),
            })
            .is_empty(),
        "pending Starmap loads should not draw synthetic fallback positions for missing layout rows"
    );
    browser.apply_starmap_layout_load_result(StarmapLayoutLoadResult {
        signature: request.signature,
        result: Ok(HashMap::from([(
            positioned_id.clone(),
            wavecrate::sample_sources::StarmapLayoutPoint {
                x: 0.25,
                y: 0.75,
                cluster_id: None,
            },
        )])),
    });

    let map_ids = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .map(|item| item.file_id)
        .collect::<Vec<_>>();

    assert_eq!(map_ids, vec![positioned_id]);
    assert!(
        !map_ids.contains(&missing_id),
        "completed Starmap loads should keep omitting missing layout rows"
    );
}

#[test]
fn starmap_color_prefers_similarity_cluster_color() {
    let cluster_color = starmap_color(
        SimilarityAspect::Spectrum,
        Some(0.5),
        Some(StarmapLayoutPoint {
            x: 0.16,
            y: 0.46,
            cluster_id: Some(1),
        }),
    );
    let aspect_color = starmap_color(SimilarityAspect::Spectrum, Some(0.5), None);

    assert_ne!(cluster_color, aspect_color);
    assert_eq!(cluster_color.a, 210);
}

#[test]
fn starmap_cluster_colors_fade_by_layout_position() {
    let left = starmap_color(
        SimilarityAspect::Spectrum,
        Some(0.5),
        Some(StarmapLayoutPoint {
            x: 0.16,
            y: 0.46,
            cluster_id: Some(1),
        }),
    );
    let nearby = starmap_color(
        SimilarityAspect::Spectrum,
        Some(0.5),
        Some(StarmapLayoutPoint {
            x: 0.20,
            y: 0.48,
            cluster_id: Some(37),
        }),
    );
    let far = starmap_color(
        SimilarityAspect::Spectrum,
        Some(0.5),
        Some(StarmapLayoutPoint {
            x: 0.84,
            y: 0.62,
            cluster_id: Some(37),
        }),
    );

    assert!(
        color_distance(left, nearby) < color_distance(left, far),
        "nearby clustered samples should have more similar colors than distant samples"
    );
}

#[test]
fn selected_starmap_position_uses_current_filtered_projection() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("kick.wav");
    std::fs::write(&kick, []).expect("write sample");
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let kick_id = kick.to_string_lossy().to_string();
    browser.select_file(kick_id.clone());
    let tags_by_file = HashMap::new();
    complete_test_starmap_layout(&mut browser, &tags_by_file, std::slice::from_ref(&kick_id));

    let position = browser.selected_starmap_position(StarmapProjection {
        tags_by_file: &tags_by_file,
        instant_audition_sample_paths: &HashSet::new(),
        preview_audition_sample_paths: &HashSet::new(),
    });

    assert!(position.is_some());
    let projection = browser.starmap_projection(StarmapProjection {
        tags_by_file: &tags_by_file,
        instant_audition_sample_paths: &HashSet::new(),
        preview_audition_sample_paths: &HashSet::new(),
    });
    let selected = projection
        .iter()
        .find(|item| item.file_id == kick_id)
        .expect("selected map item");
    assert_eq!(position, Some((selected.x, selected.y)));
    assert!(selected.focused);
}

#[test]
fn selected_starmap_position_reuses_cached_projection() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("kick.wav");
    std::fs::write(&kick, []).expect("write sample");
    let kick_id = kick.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    browser.select_file(kick_id.clone());
    browser.sample_list.starmap_layout.projection_items =
        Some(Arc::from(vec![test_starmap_item(&kick_id, 0.18, 0.82)]));
    let tags_by_file = HashMap::new();

    let position = browser.selected_starmap_position(StarmapProjection {
        tags_by_file: &tags_by_file,
        instant_audition_sample_paths: &HashSet::new(),
        preview_audition_sample_paths: &HashSet::new(),
    });

    assert_eq!(position, Some((0.18, 0.82)));
}

#[test]
fn starmap_layout_request_is_needed_only_until_pending_or_loaded() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("kick.wav");
    std::fs::write(&kick, []).expect("write sample");
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    browser.prepare_starmap_layout(&tags_by_file);

    assert!(browser.starmap_layout_load_may_need_request());
    let request = browser
        .take_starmap_layout_load_request(&tags_by_file)
        .expect("first map layout request");

    assert!(
        !browser.starmap_layout_load_may_need_request(),
        "pending layout loads should not ask the frame loop to rebuild map requests"
    );
    assert!(
        browser
            .take_starmap_layout_load_request(&tags_by_file)
            .is_none(),
        "duplicate layout requests should stay suppressed while one is pending"
    );
    browser.apply_starmap_layout_load_result(StarmapLayoutLoadResult {
        signature: request.signature,
        result: Ok(HashMap::new()),
    });
    assert!(
        !browser.starmap_layout_load_may_need_request(),
        "loaded layouts should stay quiet until the starmap layout is invalidated"
    );

    browser.invalidate_starmap_layout();
    assert!(browser.starmap_layout_load_may_need_request());
}

#[test]
fn starmap_keyboard_navigation_uses_map_position_not_list_order() {
    let root = tempfile::tempdir().expect("source root");
    let alpha = root.path().join("alpha.wav");
    let beta = root.path().join("beta.wav");
    let close_below = root.path().join("close_below.wav");
    std::fs::write(&alpha, []).expect("write alpha");
    std::fs::write(&beta, []).expect("write beta");
    std::fs::write(&close_below, []).expect("write close");
    let alpha_id = alpha.to_string_lossy().to_string();
    let beta_id = beta.to_string_lossy().to_string();
    let close_below_id = close_below.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    browser.prepare_starmap_layout(&tags_by_file);
    browser.sample_list.starmap_layout.points_by_file = HashMap::from([
        (
            alpha_id.clone(),
            StarmapLayoutPoint {
                x: 0.50,
                y: 0.50,
                cluster_id: None,
            },
        ),
        (
            beta_id.clone(),
            StarmapLayoutPoint {
                x: 0.50,
                y: 0.92,
                cluster_id: None,
            },
        ),
        (
            close_below_id.clone(),
            StarmapLayoutPoint {
                x: 0.52,
                y: 0.58,
                cluster_id: None,
            },
        ),
    ]);
    browser.select_file(alpha_id.clone());

    let down = browser.navigate_starmap_matching_tags(1, false, &tags_by_file, &HashSet::new());
    let up = browser.navigate_starmap_matching_tags(-1, false, &tags_by_file, &HashSet::new());

    assert_eq!(
        down,
        Some(close_below_id),
        "map navigation should pick the closest lower map node, not the next filename row"
    );
    assert_eq!(up, Some(alpha_id));
}

#[test]
fn starmap_keyboard_navigation_reuses_cached_projection() {
    let root = tempfile::tempdir().expect("source root");
    let alpha = root.path().join("alpha.wav");
    let beta = root.path().join("beta.wav");
    let close_below = root.path().join("close_below.wav");
    std::fs::write(&alpha, []).expect("write alpha");
    std::fs::write(&beta, []).expect("write beta");
    std::fs::write(&close_below, []).expect("write close");
    let alpha_id = alpha.to_string_lossy().to_string();
    let beta_id = beta.to_string_lossy().to_string();
    let close_below_id = close_below.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    browser.prepare_starmap_layout(&tags_by_file);
    browser.sample_list.starmap_layout.points_by_file = HashMap::from([
        (
            alpha_id.clone(),
            StarmapLayoutPoint {
                x: 0.50,
                y: 0.50,
                cluster_id: None,
            },
        ),
        (
            beta_id.clone(),
            StarmapLayoutPoint {
                x: 0.51,
                y: 0.58,
                cluster_id: None,
            },
        ),
        (
            close_below_id.clone(),
            StarmapLayoutPoint {
                x: 0.50,
                y: 0.92,
                cluster_id: None,
            },
        ),
    ]);
    browser.sample_list.starmap_layout.projection_items = Some(Arc::from(vec![
        test_starmap_item(&alpha_id, 0.50, 0.50),
        test_starmap_item(&beta_id, 0.50, 0.92),
        test_starmap_item(&close_below_id, 0.52, 0.58),
    ]));
    browser.select_file(alpha_id);

    let down = browser.navigate_starmap_matching_tags(1, false, &tags_by_file, &HashSet::new());

    assert_eq!(
        down,
        Some(close_below_id),
        "keyboard navigation should use the already prepared map projection instead of rebuilding dense items"
    );
}

#[test]
fn starmap_projection_matches_filtered_browser_listing() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("deep_kick.wav");
    let snare = root.path().join("deep_snare.wav");
    let hat = root.path().join("bright_hat.wav");
    std::fs::write(&kick, []).expect("write kick");
    std::fs::write(&snare, []).expect("write snare");
    std::fs::write(&hat, []).expect("write hat");
    let kick_id = kick.to_string_lossy().to_string();
    let snare_id = snare.to_string_lossy().to_string();
    let hat_id = hat.to_string_lossy().to_string();
    let tags_by_file = HashMap::from([
        (kick_id.clone(), vec![String::from("drum")]),
        (snare_id.clone(), vec![String::from("drum")]),
        (hat_id.clone(), vec![String::from("metal")]),
    ]);
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);

    browser.apply_name_filter_input(radiant::widgets::TextInputMessage::Changed {
        value: String::from("deep"),
    });
    browser.apply_tag_filter_input(radiant::widgets::TextInputMessage::Changed {
        value: String::from("drum"),
    });
    complete_test_starmap_layout(
        &mut browser,
        &tags_by_file,
        &[kick_id.clone(), snare_id.clone()],
    );

    let listing_ids = browser
        .browser_listing_snapshot(&tags_by_file)
        .ids()
        .to_vec();
    let map_ids = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .map(|item| item.file_id)
        .collect::<Vec<_>>();

    assert_eq!(listing_ids, vec![kick_id, snare_id]);
    assert_eq!(
        map_ids, listing_ids,
        "starmap mode must project exactly the same filtered files as list mode"
    );
}

#[test]
fn starmap_projection_uses_full_filtered_listing_not_virtual_list_window() {
    let root = tempfile::tempdir().expect("source root");
    let files = (0..32)
        .map(|index| root.path().join(format!("drum_{index:02}.wav")))
        .collect::<Vec<_>>();
    for file in &files {
        std::fs::write(file, []).expect("write sample");
    }
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    browser.apply_file_view_window_change(radiant::prelude::VirtualListWindowChange {
        offset_y: 0.0,
        row_height: 22.0,
        window: radiant::prelude::VirtualListWindow {
            total_items: 32,
            viewport_start: 0,
            viewport_end: 8,
            window_start: 0,
            window_end: 8,
        },
    });
    let tags_by_file = HashMap::new();
    let cached_sample_paths = HashSet::new();
    let file_ids = files
        .iter()
        .map(|file| file.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    complete_test_starmap_layout(&mut browser, &tags_by_file, &file_ids);

    let visible = browser.visible_samples(
        crate::native_app::sample_library::folder_browser::projection::VisibleSampleQuery {
            tags_by_file: &tags_by_file,
            cached_sample_paths: &cached_sample_paths,
        },
    );
    let map_ids = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .map(|item| item.file_id)
        .collect::<Vec<_>>();
    let listing_ids = browser
        .browser_listing_snapshot(&tags_by_file)
        .ids()
        .to_vec();

    assert!(visible.rows.len() < visible.total_count);
    assert_eq!(visible.rows.len(), 8);
    assert_eq!(visible.total_count, 32);
    assert_eq!(
        map_ids, listing_ids,
        "starmap must include the full filtered listing, not only virtualized list rows"
    );
}

#[test]
fn starmap_projection_marks_cold_long_wavs_as_preview_candidates() {
    let root = tempfile::tempdir().expect("source root");
    let short = root.path().join("short.wav");
    let long = root.path().join("long.wav");
    std::fs::write(&short, []).expect("write short sample");
    write_sparse_wav_i16(&long, 1, 1_024);
    let short_id = short.to_string_lossy().to_string();
    let long_id = long.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    complete_test_starmap_layout(
        &mut browser,
        &tags_by_file,
        &[long_id.clone(), short_id.clone()],
    );

    let cold_items = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .map(|item| {
            let audition_candidate = item.audition_candidate();
            (
                item.file_id,
                item.instant_audition_ready,
                item.preview_audition_candidate,
                audition_candidate,
            )
        })
        .collect::<Vec<_>>();
    let ready_items = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::from([long_id.clone()]),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .map(|item| {
            let audition_candidate = item.audition_candidate();
            (
                item.file_id,
                item.instant_audition_ready,
                item.preview_audition_candidate,
                audition_candidate,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        cold_items,
        vec![
            (long_id.clone(), false, true, true),
            (short_id.clone(), true, true, true)
        ]
    );
    assert_eq!(
        ready_items,
        vec![(long_id, true, true, true), (short_id, true, true, true)]
    );
}

#[test]
fn starmap_projection_marks_preview_heads_fast_ready_without_full_cache() {
    let root = tempfile::tempdir().expect("source root");
    let long = root.path().join("long.wav");
    write_sparse_wav_i16(&long, 1, 1_024);
    let long_id = long.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    complete_test_starmap_layout(&mut browser, &tags_by_file, std::slice::from_ref(&long_id));

    let item = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::from([long_id.clone()]),
        })
        .into_iter()
        .find(|item| item.file_id == long_id)
        .expect("long map item");

    assert!(!item.instant_audition_ready);
    assert!(item.preview_audition_ready);
    assert!(item.fast_audition_ready());
}

#[test]
fn starmap_projection_groups_by_enabled_similarity_aspects() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("kick.wav");
    let snare = root.path().join("snare.wav");
    std::fs::write(&kick, []).expect("write kick");
    std::fs::write(&snare, []).expect("write snare");
    let kick_id = kick.to_string_lossy().to_string();
    let snare_id = snare.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let mut aspects = [None; wavecrate_analysis::aspects::ASPECT_COUNT];
    aspects[SimilarityAspect::Spectrum.index()] = Some(0.6);
    aspects[SimilarityAspect::Timbre.index()] = Some(1.0);
    browser.set_similarity_scores_with_aspects(
        kick_id.clone(),
        HashMap::from([(snare_id.clone(), 0.9)]),
        HashMap::from([(snare_id.clone(), aspects)]),
    );
    let tags_by_file = HashMap::new();
    complete_test_starmap_layout(
        &mut browser,
        &tags_by_file,
        &[kick_id.clone(), snare_id.clone()],
    );

    let timbre_color = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .find(|item| item.file_id == snare_id.as_str())
        .expect("snare map item")
        .color;

    let mut controls = browser.similarity_controls().clone();
    controls.set_aspect_enabled(SimilarityAspect::Timbre, false);
    browser.set_similarity_controls(controls);
    let spectrum_color = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .find(|item| item.file_id == snare_id.as_str())
        .expect("snare map item after disabling timbre")
        .color;

    assert_eq!(
        (timbre_color.r, timbre_color.g, timbre_color.b),
        (255, 142, 56)
    );
    assert_eq!(
        (spectrum_color.r, spectrum_color.g, spectrum_color.b),
        (239, 216, 66)
    );
}

#[test]
fn starmap_projection_marks_all_selected_list_items() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("kick.wav");
    let snare = root.path().join("snare.wav");
    let hat = root.path().join("hat.wav");
    std::fs::write(&kick, []).expect("write kick");
    std::fs::write(&snare, []).expect("write snare");
    std::fs::write(&hat, []).expect("write hat");
    let kick_id = kick.to_string_lossy().to_string();
    let snare_id = snare.to_string_lossy().to_string();
    let hat_id = hat.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    complete_test_starmap_layout(
        &mut browser,
        &tags_by_file,
        &[kick_id.clone(), snare_id.clone(), hat_id.clone()],
    );

    browser.select_file(kick_id.clone());
    browser.select_file_with_modifiers(
        snare_id.clone(),
        radiant::widgets::PointerModifiers {
            command: true,
            ..radiant::widgets::PointerModifiers::default()
        },
    );

    let selected_map_items = browser
        .starmap_projection(StarmapProjection {
            tags_by_file: &tags_by_file,
            instant_audition_sample_paths: &HashSet::new(),
            preview_audition_sample_paths: &HashSet::new(),
        })
        .into_iter()
        .filter(|item| item.selected)
        .collect::<Vec<_>>();
    let selected_map_ids = selected_map_items
        .iter()
        .map(|item| item.file_id.clone())
        .collect::<Vec<_>>();
    let focused_map_ids = selected_map_items
        .iter()
        .filter(|item| item.focused)
        .map(|item| item.file_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(selected_map_ids, vec![kick_id, snare_id.clone()]);
    assert_eq!(focused_map_ids, vec![snare_id]);
    assert!(!selected_map_ids.contains(&hat_id));
}

#[test]
fn starmap_projection_marks_only_the_x_selected_item_for_flash() {
    let root = tempfile::tempdir().expect("source root");
    let kick = root.path().join("kick.wav");
    let snare = root.path().join("snare.wav");
    std::fs::write(&kick, []).expect("write kick");
    std::fs::write(&snare, []).expect("write snare");
    let kick_id = kick.to_string_lossy().to_string();
    let snare_id = snare.to_string_lossy().to_string();
    let mut browser =
        FolderBrowserState::from_sample_sources(&[SampleSource::new(root.path().to_path_buf())]);
    let tags_by_file = HashMap::new();
    complete_test_starmap_layout(
        &mut browser,
        &tags_by_file,
        &[kick_id.clone(), snare_id.clone()],
    );
    browser.flash_marked_item(kick_id.clone());

    let items = browser.starmap_projection(StarmapProjection {
        tags_by_file: &tags_by_file,
        instant_audition_sample_paths: &HashSet::new(),
        preview_audition_sample_paths: &HashSet::new(),
    });

    assert!(
        items
            .iter()
            .any(|item| item.file_id == kick_id && item.selection_flash)
    );
    assert!(
        items
            .iter()
            .any(|item| item.file_id == snare_id && !item.selection_flash)
    );
}

#[test]
fn starmap_status_reports_incomplete_layout_coverage() {
    let status = StarmapStatus {
        listed_count: 12,
        layout_count: 5,
        clustered_count: 2,
        cluster_color_count: 2,
    };

    assert!(status.incomplete());
    assert_eq!(
        status.label(true),
        Some(String::from("Preparing Starmap 5 / 12"))
    );
    assert_eq!(status.label(false), Some(String::from("Starmap 5 / 12")));
}

#[test]
fn complete_starmap_status_stays_silent() {
    let status = StarmapStatus {
        listed_count: 12,
        layout_count: 12,
        clustered_count: 8,
        cluster_color_count: 4,
    };

    assert!(!status.incomplete());
    assert_eq!(status.label(true), None);
}

#[test]
fn strongest_enabled_aspect_uses_similarity_strengths() {
    let mut aspects = [None; wavecrate_analysis::aspects::ASPECT_COUNT];
    aspects[SimilarityAspect::Spectrum.index()] = Some(0.2);
    aspects[SimilarityAspect::Timbre.index()] = Some(0.9);

    assert_eq!(
        strongest_enabled_aspect(
            &aspects,
            &wavecrate::sample_sources::config::SimilarityAspectSettings::default(),
        ),
        SimilarityAspect::Timbre
    );
}

fn color_distance(left: ui::Rgba8, right: ui::Rgba8) -> u16 {
    u16::from(left.r.abs_diff(right.r))
        + u16::from(left.g.abs_diff(right.g))
        + u16::from(left.b.abs_diff(right.b))
}
