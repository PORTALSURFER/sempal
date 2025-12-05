slint::slint! {
    import { ListView } from "std-widgets.slint";

    export struct SourceRow { name: string, path: string }
    export struct WavRow { name: string, path: string }

    export component HelloWorld inherits Window {
        preferred-width: 960px;
        preferred-height: 560px;
        min-width: 480px;
        min-height: 320px;
        in-out property <string> status_text: "Add a sample source to get started";
        in-out property <image> waveform;
        in-out property <float> playhead_position: 0.0;
        in-out property <bool> playhead_visible: false;
        callback seek_requested(float);
        callback close_requested();
        in-out property <[SourceRow]> sources;
        in-out property <int> selected_source: -1;
        in-out property <[WavRow]> wavs;
        callback source_selected(int);
        callback add_source();
        callback wav_clicked(int);

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

                        ListView {
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
                                }
                                ta_source := TouchArea {
                                    width: parent.width;
                                    height: parent.height;
                                    clicked => { root.source_selected(i); }
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

                                Text {
                                    text: "Waveform Viewer";
                                    horizontal-alignment: center;
                                    color: #e0e0e0;
                                    font-size: 18px;
                                    horizontal-stretch: 1;
                                }

                                Rectangle {
                                    horizontal-stretch: 1;
                                    preferred-height: 260px;
                                    min-height: 220px;
                                    vertical-stretch: 1;
                                    clip: true;

                                    Image {
                                        source: root.waveform;
                                        width: parent.width;
                                        height: parent.height;
                                        image-fit: contain;
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

                                    TouchArea {
                                        width: parent.width;
                                        height: parent.height;
                                        clicked => {
                                            root.seek_requested(self.mouse-x / self.width);
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

                                ListView {
                                    vertical-stretch: 1;
                                    min-height: 200px;
                                    for file[i] in root.wavs: Rectangle {
                                        height: 32px;
                                        horizontal-stretch: 1;
                                        background: ta_wav.pressed ? #1f1f1f : (ta_wav.has-hover ? #1a1a1a : #141414);
                                        HorizontalLayout {
                                            padding: 8px;
                                            spacing: 8px;
                                            Text {
                                                text: "[wav]";
                                                color: #c0c0c0;
                                            }
                                            Text {
                                                text: file.name;
                                                color: #e0e0e0;
                                                horizontal-alignment: left;
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
                            height: 32px;
                            background: #00000033;
                            border-width: 1px;
                            border-color: #303030;
                            border-radius: 6px;
                            Text {
                                text: root.status_text;
                                vertical-alignment: center;
                                width: parent.width;
                                height: parent.height;
                                color: #d0d0d0;
                            }
                        }
                    }
                }
            }
        }
    }
}
