slint::slint! {
    import { ListView } from "std-widgets.slint";

    export struct SourceRow { name: string, path: string }
    export struct WavRow {
        name: string,
        path: string,
        selected: bool,
        loaded: bool,
        tag_label: string,
        tag_bg: color,
        tag_fg: color,
    }

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
        in-out property <[WavRow]> wavs;
        in-out property <int> selected_wav: -1;
        in-out property <string> loaded_wav_path: "";
        in-out property <int> source_menu_index: -1;
        callback source_selected(int);
        callback source_update_requested(int);
        callback source_remove_requested(int);
        callback add_source();
        callback wav_clicked(int);
        callback scroll_sources_to(int);
        callback scroll_wavs_to(int);
        property <bool> selection_drag_active: false;
        property <bool> selection_drag_moved: false;

        VerticalLayout {
            spacing: 8px;
            padding: 0px;

            Rectangle {
                height: 36px;
                background: #181818;
                border-width: 1px;
                border-color: #303030;

                HorizontalLayout {
                    padding: 8px;
                    spacing: 8px;

                    Text {
                        text: "Sample Sources";
                        color: #e0e0e0;
                        vertical-alignment: center;
                    }

                    Rectangle {
                        height: 1px;
                        horizontal-stretch: 1;
                        background: #00000000;
                    }

                    Rectangle {
                        width: 28px;
                        height: parent.height - 8px;
                        background: #2a2a2a;
                        border-width: 1px;
                        border-color: #404040;
                        border-radius: 4px;

                        Text {
                            text: "X";
                            horizontal-alignment: center;
                            vertical-alignment: center;
                            color: #e0e0e0;
                            width: parent.width;
                            height: parent.height;
                        }

                        TouchArea {
                            width: parent.width;
                            height: parent.height;
                            clicked => { root.close_requested(); }
                        }
                    }
                }
            }

            HorizontalLayout {
                spacing: 8px;
                padding: 8px;
                vertical-stretch: 1;

                Rectangle {
                    width: 220px;
                    vertical-stretch: 1;
                    background: #101010;
                    border-width: 1px;
                    border-color: #303030;
                    border-radius: 6px;

                    VerticalLayout {
                        padding: 8px;
                        spacing: 8px;

                        HorizontalLayout {
                            spacing: 6px;
                            Text {
                                text: "Sources";
                                color: #e0e0e0;
                            }
                            Rectangle {
                                width: 20px;
                                height: 20px;
                                background: #1f8bff;
                                border-radius: 4px;
                                HorizontalLayout {
                                    padding: 0px;
                                    Text {
                                        text: "+";
                                        width: parent.width;
                                        height: parent.height;
                                        horizontal-alignment: center;
                                        vertical-alignment: center;
                                        color: #ffffff;
                                    }
                                }
                                TouchArea {
                                    width: parent.width;
                                    height: parent.height;
                                    clicked => { root.add_source(); }
                                }
                            }
                        }

                        source_list := ListView {
                            vertical-stretch: 1;
                            min-height: 200px;
                            for source[i] in root.sources: Rectangle {
                                height: 36px;
                                horizontal-stretch: 1;
                                background: ta_source.pressed ? #1f1f1f : (i == root.selected_source ? #1a2733 : (ta_source.has-hover ? #141414 : #101010));
                                border-width: i == root.selected_source ? 1px : 0px;
                                border-color: #2f6fb1;
                                HorizontalLayout {
                                    padding: 8px;
                                    spacing: 6px;
                                    Text {
                                        text: source.name;
                                        color: #e0e0e0;
                                    }
                                    Rectangle {
                                        horizontal-stretch: 1;
                                        height: 1px;
                                        background: #00000000;
                                    }
                                    Rectangle {
                                        width: 22px;
                                        height: 22px;
                                        background: #1b1b1b;
                                        border-radius: 4px;
                                        border-width: 1px;
                                        border-color: #2f2f2f;
                                        Text {
                                            text: "⋮";
                                            horizontal-alignment: center;
                                            vertical-alignment: center;
                                            color: #c8c8c8;
                                            width: parent.width;
                                            height: parent.height;
                                        }
                                        TouchArea {
                                            width: parent.width;
                                            height: parent.height;
                                            clicked => { root.source_menu_index = i; }
                                        }
                                    }
                                }
                                ta_source := TouchArea {
                                    width: parent.width;
                                    height: parent.height;
                                    clicked => {
                                        root.source_menu_index = -1;
                                        root.source_selected(i);
                                    }
                                }
                                if root.source_menu_index == i : Rectangle {
                                    x: parent.width - self.width - 8px;
                                    y: parent.height;
                                    width: 160px;
                                    background: #0f1115;
                                    border-width: 1px;
                                    border-color: #2a3a50;
                                    border-radius: 6px;
                                    z: 5;
                                    VerticalLayout {
                                        padding: 6px;
                                        spacing: 6px;
                                        Rectangle {
                                            height: 28px;
                                            background: #162235;
                                            border-radius: 4px;
                                            Text {
                                                text: "Update files";
                                                color: #d8dfe8;
                                                vertical-alignment: center;
                                                horizontal-alignment: center;
                                                width: parent.width;
                                                height: parent.height;
                                            }
                                            TouchArea {
                                                width: parent.width;
                                                height: parent.height;
                                                clicked => {
                                                    root.source_menu_index = -1;
                                                    root.source_update_requested(i);
                                                }
                                            }
                                        }
                                        Rectangle {
                                            height: 28px;
                                            background: #251919;
                                            border-radius: 4px;
                                            Text {
                                                text: "Remove source";
                                                color: #e8c8c8;
                                                vertical-alignment: center;
                                                horizontal-alignment: center;
                                                width: parent.width;
                                                height: parent.height;
                                            }
                                            TouchArea {
                                                width: parent.width;
                                                height: parent.height;
                                                clicked => {
                                                    root.source_menu_index = -1;
                                                    root.source_remove_requested(i);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
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

                        Rectangle {
                            horizontal-stretch: 1;
                            border-width: 1px;
                            border-color: #404040;
                            background: #101010;
                            border-radius: 6px;
                            VerticalLayout {
                                spacing: 8px;
                                padding: 8px;

                                HorizontalLayout {
                                    spacing: 8px;
                                    Text {
                                        text: "Waveform Viewer";
                                        horizontal-alignment: left;
                                        color: #e0e0e0;
                                        font-size: 18px;
                                        horizontal-stretch: 1;
                                    }
                                    Rectangle {
                                        width: 126px;
                                        height: 26px;
                                        border-radius: 13px;
                                        background: root.loop_enabled ? #1f2f25 : #181818;
                                        border-width: 1px;
                                        border-color: root.loop_enabled ? #3a8f64 : #303030;
                                        HorizontalLayout {
                                            padding-left: 10px;
                                            padding-right: 10px;
                                            spacing: 8px;
                                            Text {
                                                text: "Loop Playback";
                                                color: root.loop_enabled ? #7fddb4 : #c8c8c8;
                                                vertical-alignment: center;
                                                font-size: 12px;
                                            }
                                            Rectangle {
                                                width: 18px;
                                                height: 18px;
                                                border-radius: 9px;
                                                background: root.loop_enabled ? #3fbf86 : #202020;
                                                border-width: 1px;
                                                border-color: root.loop_enabled ? #62e7a3 : #404040;
                                            }
                                        }
                                        TouchArea {
                                            width: parent.width;
                                            height: parent.height;
                                            clicked => {
                                                root.loop_enabled = !root.loop_enabled;
                                                root.loop_toggled(root.loop_enabled);
                                            }
                                        }
                                    }
                                }

                                wave_area := Rectangle {
                                    horizontal-stretch: 1;
                                    preferred-height: 260px;
                                    min-height: 220px;
                                    vertical-stretch: 1;
                                    clip: true;

                                    Image {
                                        source: root.waveform;
                                        width: parent.width;
                                        height: parent.height;
                                        image-fit: fill;
                                        colorize: #00000000;
                                    }

                                    Rectangle {
                                        visible: root.playhead_visible;
                                        width: 2px;
                                        height: parent.height;
                                        x: (root.playhead_position * parent.width) - (self.width / 2);
                                        background: #3399ff;
                                        z: 1;
                                    }

                                    if root.selection_visible : selection_overlay := Rectangle {
                                        x: root.selection_start * wave_area.width;
                                        width: (root.selection_end - root.selection_start) * wave_area.width;
                                        height: parent.height;
                                        z: 3;
                                        background: #1c3f6a55;
                                        border-width: 1px;
                                        border-color: #3a82c4aa;

                                        Rectangle {
                                            width: parent.width;
                                            height: parent.height;
                                            background: #10376155;
                                        }

                                        handle_start := Rectangle {
                                            width: 4px;
                                            height: parent.height;
                                            x: -(self.width / 2);
                                            background: #4ba3ffcc;
                                            border-radius: 2px;
                                            z: 4;
                                            TouchArea {
                                                width: parent.width;
                                                height: parent.height;
                                                pointer-event(event) => {
                                                    if event.kind == PointerEventKind.down {
                                                        root.selection_handle_pressed(true);
                                                    } else if event.kind == PointerEventKind.move {
                                                        root.selection_drag_updated(
                                                            (selection_overlay.x
                                                                + handle_start.x
                                                                + self.mouse-x)
                                                                / wave_area.width,
                                                        );
                                                    } else if event.kind == PointerEventKind.up {
                                                        root.selection_drag_finished();
                                                    }
                                                }
                                            }
                                        }

                                        handle_end := Rectangle {
                                            width: 4px;
                                            height: parent.height;
                                            x: parent.width - (self.width / 2);
                                            background: #4ba3ffcc;
                                            border-radius: 2px;
                                            z: 4;
                                            TouchArea {
                                                width: parent.width;
                                                height: parent.height;
                                                pointer-event(event) => {
                                                    if event.kind == PointerEventKind.down {
                                                        root.selection_handle_pressed(false);
                                                    } else if event.kind == PointerEventKind.move {
                                                        root.selection_drag_updated(
                                                            (selection_overlay.x
                                                                + handle_end.x
                                                                + self.mouse-x)
                                                                / wave_area.width,
                                                        );
                                                    } else if event.kind == PointerEventKind.up {
                                                        root.selection_drag_finished();
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    Rectangle {
                                        visible: ta_wave.has-hover;
                                        width: 2px;
                                        height: parent.height;
                                        x: ta_wave.mouse-x - (self.width / 2);
                                        background: #66ccff;
                                        z: 2;
                                    }

                                    ta_wave := TouchArea {
                                        width: parent.width;
                                        height: parent.height;
                                        z: 1;
                                        pointer-event(event) => {
                                            if event.kind == PointerEventKind.down
                                                && event.modifiers.shift
                                            {
                                                root.selection_drag_active = true;
                                                root.selection_drag_moved = false;
                                                root.selection_drag_started(
                                                    self.mouse-x / self.width,
                                                );
                                            } else if event.kind == PointerEventKind.move
                                                && root.selection_drag_active
                                                && event.modifiers.shift
                                            {
                                                root.selection_drag_moved = true;
                                                root.selection_drag_updated(
                                                    self.mouse-x / self.width,
                                                );
                                            } else if event.kind == PointerEventKind.up {
                                                if root.selection_drag_active {
                                                    root.selection_drag_active = false;
                                                    root.selection_drag_finished();
                                                }
                                                if event.modifiers.shift
                                                    && !root.selection_drag_moved
                                                {
                                                    root.selection_clear_requested();
                                                } else if !event.modifiers.shift {
                                                    root.seek_requested(self.mouse-x / self.width);
                                                }
                                                root.selection_drag_moved = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        Rectangle {
                            vertical-stretch: 1;
                            border-width: 1px;
                            border-color: #404040;
                            background: #0f0f0f;
                            border-radius: 6px;
                            VerticalLayout {
                                padding: 8px;
                                spacing: 6px;

                                Text {
                                    text: "Wav files";
                                    color: #d0d0d0;
                                }

                                wav_list := ListView {
                                    vertical-stretch: 1;
                                    min-height: 200px;
                                    for file[i] in root.wavs: Rectangle {
                                        height: 32px;
                                        horizontal-stretch: 1;
                                        background: ta_wav.pressed ? #1f1f1f :
                                            (file.loaded ? #20344c :
                                                (file.selected ? #1d1d1d :
                                                    (ta_wav.has-hover ? #1a1a1a : #141414)));
                                        border-width: file.selected ? 1px : 0px;
                                        border-color: file.loaded ? #3a9cff : #2f6fb1;
                                        HorizontalLayout {
                                            padding: 8px;
                                            spacing: 8px;
                                            Rectangle {
                                                width: 4px;
                                                height: parent.height - 6px;
                                                background: file.loaded ? #3a9cff : (file.selected ? #2f6fb1 : #00000000);
                                                border-radius: 2px;
                                            }
                                            Text {
                                                text: file.name;
                                                color: #e0e0e0;
                                                horizontal-alignment: left;
                                                horizontal-stretch: 1;
                                            }
                                            if file.tag_label != "" : Rectangle {
                                                height: 20px;
                                                border-radius: 10px;
                                                background: file.tag_bg;
                                                HorizontalLayout {
                                                    padding-left: 8px;
                                                    padding-right: 8px;
                                                    Text {
                                                        text: file.tag_label;
                                                        color: file.tag_fg;
                                                        horizontal-alignment: center;
                                                        vertical-alignment: center;
                                                    }
                                                }
                                            }
                                        }
                                        ta_wav := TouchArea {
                                            width: parent.width;
                                            height: parent.height;
                                            clicked => { root.wav_clicked(i); }
                                        }
                                    }
                                }
                            }
                        }

                        Rectangle {
                            height: 34px;
                            background: #00000033;
                            border-width: 1px;
                            border-color: #303030;
                            border-radius: 6px;
                            HorizontalLayout {
                                padding: 8px;
                                spacing: 10px;
                                Rectangle {
                                    width: 18px;
                                    height: 18px;
                                    border-radius: 9px;
                                    background: root.status_badge_color;
                                    border-width: 1px;
                                    border-color: #00000055;
                                }
                                Text {
                                    text: root.status_badge_text;
                                    vertical-alignment: center;
                                    color: #d0d0d0;
                                }
                                Rectangle {
                                    width: 1px;
                                    height: parent.height * 0.55;
                                    background: #2d2d2d;
                                }
                                Text {
                                    text: root.status_text;
                                    vertical-alignment: center;
                                    horizontal-stretch: 1;
                                    color: #d0d0d0;
                                }
                            }
                        }
                    }
                }
            }
        }

        scroll_sources_to(index) => {
            if index < 0 { return; }
            let row_height = 36px;
            let visible = source_list.visible-height;
            let desired_top = index * row_height;
            let desired_bottom = desired_top + row_height;
            let current_top = -source_list.viewport-y;
            let margin = 6px;

            if desired_top < current_top + margin {
                let target = desired_top - margin;
                let clamped = target < 0px ? 0px : target;
                source_list.viewport-y = -clamped;
            } else if desired_bottom > current_top + visible - margin {
                let target = desired_bottom - visible + margin;
                let clamped = target < 0px ? 0px : target;
                source_list.viewport-y = -clamped;
            }
        }

        scroll_wavs_to(index) => {
            if index < 0 { return; }
            let row_height = 32px;
            let visible = wav_list.visible-height;
            let desired_top = index * row_height;
            let desired_bottom = desired_top + row_height;
            let current_top = -wav_list.viewport-y;
            let margin = 6px;

            if desired_top < current_top + margin {
                let target = desired_top - margin;
                let clamped = target < 0px ? 0px : target;
                wav_list.viewport-y = -clamped;
            } else if desired_bottom > current_top + visible - margin {
                let target = desired_bottom - visible + margin;
                let clamped = target < 0px ? 0px : target;
                wav_list.viewport-y = -clamped;
            }
        }

    }
}
