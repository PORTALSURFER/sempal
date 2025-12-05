slint::slint! {
    import { TopBar } from "ui/top_bar.slint";
    import { SourcePanel } from "ui/source_panel.slint";
    import { WaveformPanel } from "ui/waveform_panel.slint";
    import { WavListPanel } from "ui/wav_list_panel.slint";
    import { StatusBar } from "ui/status_bar.slint";
    import { SourceRow, WavRow } from "ui/types.slint";

    export component HelloWorld inherits Window {
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
        callback wav_clicked(string);
        callback scroll_sources_to(int);
        callback scroll_wavs_to(int, int);
        property <bool> selection_drag_active: false;
        property <bool> selection_drag_moved: false;

        VerticalLayout {
            spacing: 8px;
            padding: 0px;

            TopBar {
                close_requested => root.close_requested();
            }

            HorizontalLayout {
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

                Rectangle {
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
                            selected_trash <=> root.selected_trash;
                            selected_neutral <=> root.selected_neutral;
                            selected_keep <=> root.selected_keep;
                            loaded_wav_path <=> root.loaded_wav_path;
                            wav_clicked(path) => root.wav_clicked(path);
                        }

                        StatusBar {
                            status_text <=> root.status_text;
                            status_badge_text <=> root.status_badge_text;
                            status_badge_color <=> root.status_badge_color;
                        }
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
    }
}
