use super::super::test_support::dummy_controller;
use crate::app_dirs::ConfigBaseGuard;
use rusqlite::params;
use tempfile::tempdir;

#[test]
fn tf_labels_crud_and_anchor_updates() {
    let config_dir = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());
    let (mut controller, _source) = dummy_controller();

    let label = controller
        .create_tf_label("Clap", 0.75, 0.1, 3)
        .unwrap();
    let labels = controller.list_tf_labels().unwrap();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].name, "Clap");

    controller
        .update_tf_label(&label.label_id, "Clap Tight", 0.8, 0.2, 2)
        .unwrap();
    let labels = controller.list_tf_labels().unwrap();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].name, "Clap Tight");
    assert_eq!(labels[0].threshold, 0.8);
    assert_eq!(labels[0].gap, 0.2);
    assert_eq!(labels[0].topk, 2);

    let sample_id = "source::Pack/a.wav";
    insert_sample(sample_id);

    let anchor = controller
        .add_tf_anchor(&label.label_id, sample_id, 1.0)
        .unwrap();
    let anchors = controller.list_tf_anchors(&label.label_id).unwrap();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].weight, 1.0);

    let anchor_again = controller
        .add_tf_anchor(&label.label_id, sample_id, 0.5)
        .unwrap();
    assert_eq!(anchor_again.anchor_id, anchor.anchor_id);
    let anchors = controller.list_tf_anchors(&label.label_id).unwrap();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].weight, 0.5);

    controller.update_tf_anchor(&anchor.anchor_id, 0.25).unwrap();
    let anchors = controller.list_tf_anchors(&label.label_id).unwrap();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].weight, 0.25);

    controller.delete_tf_anchor(&anchor.anchor_id).unwrap();
    let anchors = controller.list_tf_anchors(&label.label_id).unwrap();
    assert!(anchors.is_empty());

    controller.delete_tf_label(&label.label_id).unwrap();
    let labels = controller.list_tf_labels().unwrap();
    assert!(labels.is_empty());
}

#[test]
fn tf_label_validation_rejects_invalid_fields() {
    let config_dir = tempdir().unwrap();
    let _guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());
    let (mut controller, _source) = dummy_controller();

    let err = controller.create_tf_label("", 0.75, 0.1, 3).unwrap_err();
    assert!(err.contains("Label name"));

    let err = controller.create_tf_label("Clap", -0.1, 0.1, 3).unwrap_err();
    assert!(err.contains("threshold"));

    let err = controller.create_tf_label("Clap", 0.75, -0.1, 3).unwrap_err();
    assert!(err.contains("gap"));

    let err = controller.create_tf_label("Clap", 0.75, 0.1, 0).unwrap_err();
    assert!(err.contains("topk"));

    let label = controller
        .create_tf_label("Valid", 0.75, 0.1, 3)
        .unwrap();
    let err = controller.add_tf_anchor(&label.label_id, "sample::a.wav", 0.0).unwrap_err();
    assert!(err.contains("weight"));
}

fn insert_sample(sample_id: &str) {
    let conn = crate::sample_sources::library::open_connection().unwrap();
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns)
         VALUES (?1, ?2, ?3, ?4)",
        params![sample_id, "hash", 1, 1],
    )
    .unwrap();
}
