slint::slint! {
    import { TopBar } from "ui/top_bar.slint";
    import { SourcePanel } from "ui/source_panel.slint";
    import { WaveformPanel } from "ui/waveform_panel.slint";
    import { WavListPanel } from "ui/wav_list_panel.slint";
    import { StatusBar } from "ui/status_bar.slint";
    import { CollectionPanel } from "ui/collection_panel.slint";
    import { CollectionRow, CollectionSampleRow, SourceRow, WavRow } from "ui/types.slint";

    export component Sempal inherits Window {
        preferred-width: 960px;
        preferred-height: 560px;
        min-width: 480px;
        min-height: 320px;
        in-out property <string> status_text: "Add a sample source to get started";
        in-out property <string> status_badge_text: "Idle";
        in-out property <color> status_badge_color: #2a2a2a;
        in-out property <image> waveform;
        in-out property <float> playhead_position: 0.0;
        in-out property <bool> playhead_visible: false;
        in-out property <bool> selection_visible: false;
        in-out property <float> selection_start: 0.0;
        in-out property <float> selection_end: 0.0;
        in-out property <bool> loop_enabled: false;
        callback seek_requested(float);
        callback selection_drag_started(float);
        callback selection_drag_updated(float);
        callback selection_drag_finished();
        callback selection_clear_requested();
        callback selection_handle_pressed(bool);
        callback loop_toggled(bool);
        callback close_requested();
        in-out property <[SourceRow]> sources;
        in-out property <int> selected_source: -1;
        in-out property <[CollectionRow]> collections;
        in-out property <[CollectionSampleRow]> collection_samples;
        in-out property <int> selected_collection: -1;
        in-out property <bool> collections_enabled: true;
        in-out property <string> dragging_sample_path: "";
        in-out property <string> drag_preview_label: "";
        in-out property <length> drag_preview_x: 0px;
        in-out property <length> drag_preview_y: 0px;
        in-out property <bool> drag_preview_visible: false;
        in-out property <[WavRow]> wavs_trash;
        in-out property <[WavRow]> wavs_neutral;
        in-out property <[WavRow]> wavs_keep;
        in-out property <int> selected_trash: -1;
        in-out property <int> selected_neutral: -1;
        in-out property <int> selected_keep: -1;
        in-out property <string> loaded_wav_path: "";
        in-out property <int> source_menu_index: -1;
        callback source_selected(int);
        callback source_update_requested(int);
        callback source_remove_requested(int);
        callback add_source();
        callback add_collection();
        callback collection_selected(int);
        callback wav_clicked(string);
        callback scroll_sources_to(int);
        callback scroll_wavs_to(int, int);
        callback sample_dropped_on_collection(string, string);
        callback sample_drop_attempt(string, length, length) -> bool;
        callback sample_drag_hover(string, length, length, bool);
        property <bool> selection_drag_active: false;
        property <bool> selection_drag_moved: false;

        VerticalLayout {
            spacing: 8px;
            padding: 0px;

            TopBar {
                close_requested => root.close_requested();
            }

            main_row := HorizontalLayout {
                spacing: 8px;
                padding: 8px;
                vertical-stretch: 1;

                sources_panel := SourcePanel {
                    sources <=> root.sources;
                    selected_source <=> root.selected_source;
                    source_menu_index <=> root.source_menu_index;
                    add_source => root.add_source();
                    source_selected(i) => root.source_selected(i);
                    source_update_requested(i) => root.source_update_requested(i);
                    source_remove_requested(i) => root.source_remove_requested(i);
                }

                center_panel := Rectangle {
                    horizontal-stretch: 1;
                    vertical-stretch: 1;
                    background: #0d0d0d;
                    border-width: 1px;
                    border-color: #303030;
                    border-radius: 6px;

                    VerticalLayout {
                        spacing: 10px;
                        padding: 12px;
                        vertical-stretch: 1;

                        WaveformPanel {
                            waveform <=> root.waveform;
                            playhead_position <=> root.playhead_position;
                            playhead_visible <=> root.playhead_visible;
                            selection_visible <=> root.selection_visible;
                            selection_start <=> root.selection_start;
                            selection_end <=> root.selection_end;
                            loop_enabled <=> root.loop_enabled;
                            selection_drag_active <=> root.selection_drag_active;
                            selection_drag_moved <=> root.selection_drag_moved;
                            seek_requested(pos) => root.seek_requested(pos);
                            selection_drag_started(pos) => root.selection_drag_started(pos);
                            selection_drag_updated(pos) => root.selection_drag_updated(pos);
                            selection_drag_finished => root.selection_drag_finished();
                            selection_clear_requested => root.selection_clear_requested();
                            selection_handle_pressed(is_start) => root.selection_handle_pressed(is_start);
                            loop_toggled(enabled) => root.loop_toggled(enabled);
                        }

                        wavs_panel := WavListPanel {
                            wavs_trash <=> root.wavs_trash;
                            wavs_neutral <=> root.wavs_neutral;
                            wavs_keep <=> root.wavs_keep;
                            dragging_sample_path <=> root.dragging_sample_path;
                            drag_preview_label <=> root.drag_preview_label;
                            drag_preview_x <=> root.drag_preview_x;
                            drag_preview_y <=> root.drag_preview_y;
                            drag_preview_visible <=> root.drag_preview_visible;
                            global_offset_x: main_row.x + center_panel.x + wavs_panel.x;
                            global_offset_y: main_row.y + center_panel.y + wavs_panel.y;
                            selected_trash <=> root.selected_trash;
                            selected_neutral <=> root.selected_neutral;
                            selected_keep <=> root.selected_keep;
                            loaded_wav_path <=> root.loaded_wav_path;
                            z: 10;
                        wav_clicked(path) => root.wav_clicked(path);
                        drop_attempt(path, x, y) => root.sample_drop_attempt(path, x, y);
                        drag_hover(path, x, y, active) => root.sample_drag_hover(path, x, y, active);
                    }

                        StatusBar {
                            status_text <=> root.status_text;
                            status_badge_text <=> root.status_badge_text;
                            status_badge_color <=> root.status_badge_color;
                        }
                    }
                }

                collection_panel := CollectionPanel {
                    collections <=> root.collections;
                    collection_samples <=> root.collection_samples;
                    selected_collection <=> root.selected_collection;
                    collections_enabled <=> root.collections_enabled;
                    dragging_sample_path <=> root.dragging_sample_path;
                    drag_preview_label <=> root.drag_preview_label;
                    drag_preview_x <=> root.drag_preview_x;
                    drag_preview_y <=> root.drag_preview_y;
                    drag_preview_visible <=> root.drag_preview_visible;
                    add_collection => root.add_collection();
                    collection_selected(i) => root.collection_selected(i);
                    sample_dropped_on_collection(id, path) => {
                        root.sample_dropped_on_collection(id, path);
                    }
                }
            }
        }

        scroll_sources_to(index) => {
            if index < 0 { return; }
            sources_panel.scroll_to(index);
        }

        scroll_wavs_to(tag, index) => {
            if index < 0 { return; }
            wavs_panel.scroll_to(tag, index);
        }

        sample_drag_hover(path, x, y, active) => {
            collection_panel.update_drag_hover(path, x, y, active);
        }

        sample_drop_attempt(path, x, y) => {
            if collection_panel.try_drop(path, x, y) {
                root.drag_preview_visible = false;
                root.dragging_sample_path = "";
                collection_panel.update_drag_hover("", 0px, 0px, false);
                return true;
            }
            collection_panel.update_drag_hover("", 0px, 0px, false);
            false
        }

        if root.drag_preview_visible : Rectangle {
            width: 180px;
            height: 34px;
            x: root.drag_preview_x;
            y: root.drag_preview_y;
            z: 100;
            background: #1a2733ee;
            border-width: 1px;
            border-color: #2f6fb1;
            border-radius: 6px;
            HorizontalLayout {
                padding: 10px;
                spacing: 8px;
                Rectangle {
                    width: 8px;
                    height: 8px;
                    background: #5ab0ff;
                    border-radius: 4px;
                }
                Text {
                    text: root.drag_preview_label != "" ? root.drag_preview_label : "Sample";
                    color: #e0e0e0;
                    horizontal-stretch: 1;
                }
            }
        }
    }
}
