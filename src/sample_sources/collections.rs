use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::SourceId;

/// Identifier for a user-created collection of samples.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CollectionId(String);

impl CollectionId {
    /// Create a new unique collection identifier.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Borrow the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CollectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Link a sample (by source and relative path) to a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionMember {
    pub source_id: SourceId,
    pub relative_path: PathBuf,
}

/// User-managed grouping of samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub id: CollectionId,
    pub name: String,
    #[serde(default)]
    pub members: Vec<CollectionMember>,
}

impl Collection {
    /// Create a new collection with an empty member list.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: CollectionId::new(),
            name: name.into(),
            members: Vec::new(),
        }
    }

    /// True when the collection already contains the given sample.
    pub fn contains(&self, source_id: &SourceId, relative_path: &PathBuf) -> bool {
        self.members
            .iter()
            .any(|m| &m.source_id == source_id && &m.relative_path == relative_path)
    }

    /// Add a sample to the collection unless it already exists.
    pub fn add_member(&mut self, source_id: SourceId, relative_path: PathBuf) -> bool {
        if self.contains(&source_id, &relative_path) {
            return false;
        }
        self.members.push(CollectionMember {
            source_id,
            relative_path,
        });
        true
    }

    /// Drop any members that belong to the provided source id.
    pub fn prune_source(&mut self, source_id: &SourceId) {
        self.members.retain(|member| &member.source_id != source_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_path(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn collections_deduplicate_members() {
        let id = SourceId::new();
        let mut collection = Collection::new("Test");
        let first = collection.add_member(id.clone(), sample_path("one.wav"));
        let second = collection.add_member(id.clone(), sample_path("one.wav"));
        assert!(first);
        assert!(!second);
        assert_eq!(collection.members.len(), 1);
    }

    #[test]
    fn pruning_removes_all_members_from_source() {
        let id = SourceId::new();
        let other = SourceId::new();
        let mut collection = Collection::new("Test");
        collection.add_member(id.clone(), sample_path("one.wav"));
        collection.add_member(other.clone(), sample_path("two.wav"));
        collection.prune_source(&id);
        assert_eq!(collection.members.len(), 1);
        assert_eq!(collection.members[0].source_id, other);
    }
}
