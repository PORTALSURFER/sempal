use super::*;

#[test]
fn pack_id_uses_source_and_folder_depth() {
    assert_eq!(pack_id_for_sample_id("s::Pack/Kit/a.wav", 1).as_deref(), Some("s/Pack"));
    assert_eq!(
        pack_id_for_sample_id("s::Pack/Kit/a.wav", 2).as_deref(),
        Some("s/Pack/Kit")
    );
    assert_eq!(pack_id_for_sample_id("s::a.wav", 1).as_deref(), Some("s"));
}

#[test]
fn split_is_deterministic() {
    let a = split_for_pack_id("s/Pack", "seed", 0.1, 0.1).unwrap();
    let b = split_for_pack_id("s/Pack", "seed", 0.1, 0.1).unwrap();
    assert_eq!(a, b);
}

#[test]
fn split_rejects_invalid_fractions() {
    let err = split_for_pack_id("p", "seed", 0.9, 0.2).unwrap_err();
    matches!(err, ExportError::InvalidSplitFractions);
}

