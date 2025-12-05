slint::slint! {
    import { ListView } from "std-widgets.slint";

    struct FileRow { name: string, is_dir: bool }

    export component HelloWorld inherits Window {
        preferred-width: 720px;
        preferred-height: 420px;
        min-width: 320px;
        min-height: 240px;
        in-out property <string> status_text: "Drop a .wav file";
        in-out property <image> waveform;
        in-out property <float> playhead_position: 0.0;
        in-out property <bool> playhead_visible: false;
        callback seek_requested(float);
        callback close_requested();
        in-out property <[string]> disks;
        in-out property <int> selected_disk: 0;
        in-out property <[FileRow]> files;
        callback disk_selected(int);
        callback file_clicked(int);
        callback go_up_directory();

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
                        text: "Waveform Viewer";
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

            Rectangle {
                height: 44px;
                background: #121212;
                border-color: #303030;
                border-width: 1px;
                HorizontalLayout {
                    padding: 8px;
                    spacing: 8px;
                    for disk[i] in root.disks: Rectangle {
                        height: 28px;
                        background: i == root.selected_disk ? #2a2a2a : #1a1a1a;
                        border-width: 1px;
                        border-color: #404040;
                        border-radius: 4px;
                        HorizontalLayout {
                            padding: 6px;
                            Text {
                                text: disk;
                                color: #e0e0e0;
                            }
                        }
                        TouchArea {
                            height: parent.height;
                            clicked => { root.disk_selected(i); }
                        }
                    }
                }
            }

            VerticalLayout {
                spacing: 8px;
                padding: 12px;
                vertical-stretch: 1;

                Rectangle {
                    horizontal-stretch: 1;
                    border-width: 1px;
                    border-color: #404040;
                    background: #101010;
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
                            min-height: 260px;
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
                    height: 1px;
                    background: #202020;
                }

                Rectangle {
                    border-width: 1px;
                    border-color: #404040;
                    background: #0f0f0f;
                    vertical-stretch: 1;
                    VerticalLayout {
                        padding: 8px;
                        spacing: 6px;

                        Rectangle {
                            height: 32px;
                            background: #1a1a1a;
                            border-width: 1px;
                            border-color: #303030;
                            border-radius: 4px;
                            HorizontalLayout {
                                padding: 8px;
                                spacing: 8px;
                                Text {
                                    text: "Go up one level";
                                    color: #d0d0d0;
                                }
                            }
                            TouchArea {
                                height: parent.height;
                                clicked => { root.go_up_directory(); }
                            }
                        }

                        ListView {
                            vertical-stretch: 1;
                            min-height: 200px;
                            for file[i] in root.files: Rectangle {
                                height: 32px;
                                horizontal-stretch: 1;
                                background: ta.pressed ? #1f1f1f : (ta.has-hover ? #1a1a1a : #141414);
                                HorizontalLayout {
                                    padding: 8px;
                                    spacing: 8px;
                                    Text {
                                        text: file.is_dir ? "[dir]" : "[wav]";
                                        color: #c0c0c0;
                                    }
                                    Text {
                                        text: file.name;
                                        color: #e0e0e0;
                                        horizontal-alignment: left;
                                    }
                                }
                                ta := TouchArea {
                                    width: parent.width;
                                    height: parent.height;
                                    clicked => { root.file_clicked(i); }
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
