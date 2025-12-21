use std::collections::{BTreeMap, HashMap};

use super::samples::TrainingSample;

/// Collect class labels in deterministic order.
pub(super) fn collect_class_ids(samples: &[TrainingSample]) -> Vec<String> {
    let mut class_set = BTreeMap::new();
    for sample in samples {
        class_set.entry(sample.class_id.clone()).or_insert(());
    }
    class_set.keys().cloned().collect()
}

/// Build a class -> index lookup for model training.
pub(super) fn class_index_map(classes: &[String]) -> HashMap<String, usize> {
    classes
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, class_id)| (class_id, idx))
        .collect()
}
