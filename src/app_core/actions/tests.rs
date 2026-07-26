use super::{
    GUI_ACTION_CATALOG, GuiDispatchPolicy, GuiEffectClass, GuiSurface, action_catalog_entry_by_id,
    action_kind, representative_action_for_kind,
};
use crate::app_core::ui_bridge::{
    InteractionActionClass, InvalidationSource, catalog_dirty_source, catalog_interaction_class,
    catalog_is_immediate_waveform_preview_action, catalog_prefers_targeted_invalidation,
    catalog_uses_local_model_pull_fast_path,
};
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn catalog_contains_every_action_kind_exactly_once() {
    let mut seen = BTreeSet::new();
    for kind in GUI_ACTION_CATALOG.iter().map(|entry| entry.kind) {
        assert!(seen.insert(kind), "duplicate catalog action kind: {kind:?}");
    }
    assert_eq!(GUI_ACTION_CATALOG.len(), seen.len());
}

#[test]
fn catalog_action_ids_are_unique_and_resolvable() {
    let mut ids = BTreeSet::new();
    for entry in GUI_ACTION_CATALOG {
        assert!(
            ids.insert(entry.action_id),
            "duplicate action id {}",
            entry.action_id
        );
        assert_eq!(action_catalog_entry_by_id(entry.action_id), Some(entry));
    }
}

#[test]
fn every_catalog_entry_declares_required_coverage() {
    for entry in GUI_ACTION_CATALOG {
        assert!(
            !entry.coverage_layers.is_empty(),
            "catalog entry {} is missing coverage layers",
            entry.action_id
        );
        let mut layers = BTreeSet::new();
        for layer in entry.coverage_layers {
            assert!(
                layers.insert(layer),
                "catalog entry {} repeats coverage layer {:?}",
                entry.action_id,
                layer
            );
        }
    }
}

#[test]
fn representative_actions_round_trip_through_kind_matcher() {
    for kind in GUI_ACTION_CATALOG.iter().map(|entry| entry.kind) {
        let action = representative_action_for_kind(kind);
        assert_eq!(action_kind(&action), kind);
    }
}

#[test]
fn catalog_domain_views_are_exposed_for_action_ownership_tests() {
    let covered = crate::app_core::actions::GUI_ACTION_CATALOG_DOMAINS
        .iter()
        .flat_map(|domain| crate::app_core::actions::action_catalog_entries_by_domain(*domain))
        .map(|entry| entry.kind)
        .collect::<BTreeSet<_>>();
    let all = GUI_ACTION_CATALOG
        .iter()
        .map(|entry| entry.kind)
        .collect::<BTreeSet<_>>();

    assert_eq!(covered, all);
}

#[test]
fn every_history_enabled_catalog_entry_has_a_transaction_handler() {
    for entry in GUI_ACTION_CATALOG {
        assert!(
            crate::app::controller::catalog_history_handler_supported(
                entry.kind,
                entry.history_policy,
            ),
            "catalog history policy {:?} for {} has no controller transaction handler",
            entry.history_policy,
            entry.action_id
        );
    }
}

#[test]
fn profiled_interaction_catalog_entries_match_expected_surfaces_and_dirty_sources() {
    for entry in GUI_ACTION_CATALOG {
        let Some(class) = catalog_interaction_class(entry.kind) else {
            continue;
        };
        match class {
            InteractionActionClass::Wheel => {
                assert_eq!(entry.surface, GuiSurface::Browser);
                assert_eq!(
                    catalog_dirty_source(entry.kind).map(|(node, _)| node),
                    Some(InvalidationSource::Browser),
                );
            }
            InteractionActionClass::MapPanProxy => {
                assert_eq!(entry.surface, GuiSurface::Map);
                assert_eq!(
                    catalog_dirty_source(entry.kind).map(|(node, _)| node),
                    Some(InvalidationSource::Map),
                );
            }
            InteractionActionClass::Waveform => {
                assert!(
                    matches!(entry.surface, GuiSurface::Waveform | GuiSurface::Transport),
                    "waveform-profiled action {} should stay on waveform/transport surfaces",
                    entry.action_id
                );
                assert!(
                    matches!(
                        catalog_dirty_source(entry.kind).map(|(node, _)| node),
                        Some(InvalidationSource::Waveform)
                            | Some(InvalidationSource::Transport)
                            | None
                    ),
                    "waveform-profiled action {} should keep waveform/transport/queued dirty semantics",
                    entry.action_id
                );
            }
            InteractionActionClass::Volume => {
                assert_eq!(entry.surface, GuiSurface::Transport);
                assert_eq!(
                    catalog_dirty_source(entry.kind).map(|(node, _)| node),
                    Some(InvalidationSource::Transport),
                );
            }
        }
    }
}

#[test]
fn targeted_invalidation_catalog_entries_stay_on_sidebar_or_browser_surfaces() {
    for entry in GUI_ACTION_CATALOG {
        if !catalog_prefers_targeted_invalidation(entry.kind) {
            continue;
        }
        assert!(
            matches!(entry.surface, GuiSurface::Browser | GuiSurface::Sources),
            "targeted invalidation action {} should stay on browser/sidebar surfaces",
            entry.action_id
        );
        assert_eq!(
            catalog_dirty_source(entry.kind).map(|(node, _)| node),
            Some(InvalidationSource::Browser),
        );
    }
}

#[test]
fn immediate_waveform_preview_catalog_entries_stay_on_waveform_surface() {
    for entry in GUI_ACTION_CATALOG {
        if !catalog_is_immediate_waveform_preview_action(entry.kind) {
            continue;
        }
        assert_eq!(entry.surface, GuiSurface::Waveform);
        if let Some((node, _)) = catalog_dirty_source(entry.kind) {
            assert_eq!(node, InvalidationSource::Waveform);
        }
    }
}

#[test]
fn local_model_pull_fast_path_catalog_entries_remain_ui_only_actions() {
    for entry in GUI_ACTION_CATALOG {
        if !catalog_uses_local_model_pull_fast_path(entry.kind) {
            continue;
        }
        assert!(matches!(
            entry.effect_class,
            GuiEffectClass::Projection | GuiEffectClass::StateOnly
        ));
    }
}

#[test]
fn runtime_internal_waveform_shift_actions_are_not_public_dispatch() {
    for action_id in [
        "begin_waveform_selection_shift",
        "begin_waveform_edit_selection_shift",
    ] {
        let entry = action_catalog_entry_by_id(action_id).expect("catalog entry");
        assert_eq!(entry.dispatch_policy, GuiDispatchPolicy::RuntimeInternal);
    }
}

#[test]
fn native_app_default_keeps_wavecrate_product_labels_owned_in_app_core() {
    let model = crate::app_core::actions::NativeAppModel::default();

    assert_eq!(model.columns[1].title, "Samples");
    assert_eq!(model.browser_chrome.samples_tab_label, "Samples");
    assert_eq!(model.browser_chrome.map_tab_label, "Starmap");
    assert_eq!(model.browser_chrome.search_placeholder, "Search samples");
}

#[test]
fn post_cutover_compatibility_facades_are_absent() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(
        !manifest_dir.join("src/compat_app_contract.rs").exists(),
        "the temporary local runtime-contract facade should be deleted"
    );
    assert!(
        !manifest_dir
            .join("../radiant/src/compat/legacy_shell")
            .exists(),
        "Radiant's removed legacy compatibility tree must stay absent"
    );
}

#[test]
fn native_projection_dtos_keep_product_defaults_and_generic_primitives() {
    let model = crate::app_core::actions::NativeAppModel::default();

    assert_eq!(model.columns[1].title, "Samples");
    assert_eq!(model.browser_chrome.samples_tab_label, "Samples");
    assert_eq!(model.browser_chrome.map_tab_label, "Starmap");
    assert_eq!(model.browser_chrome.search_placeholder, "Search samples");
    assert_eq!(
        model
            .sources
            .folder_pane(crate::app_core::actions::NativeFolderPaneIdModel::Upper),
        &model.sources.upper_folder_pane
    );
    assert_eq!(
        model
            .sources
            .folder_pane(crate::app_core::actions::NativeFolderPaneIdModel::Lower),
        &model.sources.lower_folder_pane
    );

    let preferences = model.options_panel.preference_state();
    assert_eq!(preferences.toggles.len(), 4);
    assert_eq!(model.waveform.timeline_surface().markers.len(), 0);
    assert!(model.waveform_chrome.signal_tools().markers_visible);
}
