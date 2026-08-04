//! Source-owned durable watcher authority for replay admission.

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;

use crate::sample_sources::reconciliation::{
    BackendStreamIdentity, ReplayPriorToken, RootIdentity, WatcherGeneration,
};
use crate::sample_sources::{SourceDatabase, SourceId};

use super::{META_SOURCE_WATCHER_CHECKPOINT, SourceDbError};

const CHECKPOINT_FORMAT_VERSION: u64 = 3;
const CHECKPOINT_FIELDS: [&str; 8] = [
    "root_identity",
    "event_id",
    "format_version",
    "source_id",
    "lifecycle_generation",
    "source_revision",
    "cause",
    "continuity_proof",
];
const CONTINUITY_PROOF_FIELDS: [&str; 7] = [
    "root_identity",
    "backend",
    "backend_device",
    "watcher_generation",
    "replay_coverage_start_event_id",
    "replay_coverage_end_event_id",
    "acknowledged_end_event_id",
];
const FSEVENTS_STREAM_PREFIX: &[u8] = b"fsevents:";

/// A construction capability that can only be created after this module validates DB metadata.
pub(crate) struct ValidatedReplayAuthority {
    source_id: SourceId,
    root_identity: RootIdentity,
    backend_stream_identity: BackendStreamIdentity,
    watcher_generation: WatcherGeneration,
    acknowledged_sequence: u64,
}

impl ValidatedReplayAuthority {
    fn new(
        source_id: SourceId,
        root_identity: RootIdentity,
        backend_stream_identity: BackendStreamIdentity,
        watcher_generation: WatcherGeneration,
        acknowledged_sequence: u64,
    ) -> Self {
        Self {
            source_id,
            root_identity,
            backend_stream_identity,
            watcher_generation,
            acknowledged_sequence,
        }
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        SourceId,
        RootIdentity,
        BackendStreamIdentity,
        WatcherGeneration,
        u64,
    ) {
        (
            self.source_id,
            self.root_identity,
            self.backend_stream_identity,
            self.watcher_generation,
            self.acknowledged_sequence,
        )
    }
}

/// Read a durable replay prior from the source database's existing watcher checkpoint.
impl SourceDatabase {
    /// Return opaque replay authority only when the stored checkpoint matches this source lane.
    ///
    /// The checkpoint parser is intentionally strict and fail-closed. In particular, legacy,
    /// proofless, malformed, unknown, duplicated, or identity-mismatched metadata returns
    /// `Ok(None)`, while database access failures retain their original `SourceDbError`.
    pub fn read_durable_replay_prior(
        &self,
        expected_source_id: &SourceId,
        expected_root_identity: &RootIdentity,
        expected_watcher_generation: WatcherGeneration,
    ) -> Result<Option<ReplayPriorToken>, SourceDbError> {
        let Some(value) = self.get_metadata(META_SOURCE_WATCHER_CHECKPOINT)? else {
            return Ok(None);
        };
        Ok(parse_durable_replay_prior(
            &value,
            expected_source_id,
            expected_root_identity,
            expected_watcher_generation,
        ))
    }
}

fn parse_durable_replay_prior(
    value: &str,
    expected_source_id: &SourceId,
    expected_root_identity: &RootIdentity,
    expected_watcher_generation: WatcherGeneration,
) -> Option<ReplayPriorToken> {
    let object = parse_object(value).ok()?;
    if !has_exact_fields(&object, &CHECKPOINT_FIELDS) {
        return None;
    }

    let root_identity = string_field(&object, "root_identity")?;
    let event_id = u64_field(&object, "event_id")?;
    if u64_field(&object, "format_version")? != CHECKPOINT_FORMAT_VERSION
        || string_field(&object, "source_id")? != expected_source_id.as_str()
        || root_identity.as_bytes() != expected_root_identity.as_bytes()
        || u64_field(&object, "lifecycle_generation")? != expected_watcher_generation.get()
        || u64_field(&object, "source_revision").is_none()
        || string_field(&object, "cause")? != "targeted_replay"
    {
        return None;
    }

    let proof = object.get("continuity_proof")?.as_object()?;
    if !has_exact_fields(proof, &CONTINUITY_PROOF_FIELDS) {
        return None;
    }
    let proof_root_identity = string_field(proof, "root_identity")?;
    let backend = string_field(proof, "backend")?;
    let backend_device = u64_field(proof, "backend_device")?;
    let replay_start = u64_field(proof, "replay_coverage_start_event_id")?;
    let replay_end = u64_field(proof, "replay_coverage_end_event_id")?;
    let acknowledged_end = u64_field(proof, "acknowledged_end_event_id")?;
    if proof_root_identity != root_identity
        || backend != "fsevents"
        || backend_device == 0
        || u64_field(proof, "watcher_generation")? == 0
        || replay_start > replay_end
        || replay_end != acknowledged_end
        || acknowledged_end != event_id
    {
        return None;
    }

    // Keep the backend discriminator in the opaque identity so a future native replay adapter
    // cannot accidentally treat a notify/process-local stream id as FSEvents authority.
    let mut stream_identity =
        Vec::with_capacity(FSEVENTS_STREAM_PREFIX.len() + std::mem::size_of::<u64>());
    stream_identity.extend_from_slice(FSEVENTS_STREAM_PREFIX);
    stream_identity.extend_from_slice(&backend_device.to_be_bytes());
    Some(ReplayPriorToken::from_validated_durable_authority(
        ValidatedReplayAuthority::new(
            expected_source_id.clone(),
            expected_root_identity.clone(),
            BackendStreamIdentity::from_bytes(stream_identity),
            expected_watcher_generation,
            event_id,
        ),
    ))
}

struct StrictJsonValue(Value);

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonValueVisitor)
    }
}

struct StrictJsonValueVisitor;

impl<'de> Visitor<'de> for StrictJsonValueVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object fields")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let number = serde_json::Number::from_f64(value)
            .ok_or_else(|| E::custom("non-finite JSON number"))?;
        Ok(StrictJsonValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonValueVisitor)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictJsonValue>()? {
            values.push(value.0);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom("duplicate checkpoint field"));
            }
            object.insert(key, map.next_value::<StrictJsonValue>()?.0);
        }
        Ok(StrictJsonValue(Value::Object(object)))
    }
}

fn parse_object(value: &str) -> Result<Map<String, Value>, String> {
    let StrictJsonValue(value) =
        serde_json::from_str::<StrictJsonValue>(value).map_err(|error| error.to_string())?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| String::from("checkpoint must be an object"))
}

fn has_exact_fields(object: &Map<String, Value>, fields: &[&str]) -> bool {
    object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field))
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field)?.as_str()
}

fn u64_field(object: &Map<String, Value>, field: &str) -> Option<u64> {
    object.get(field)?.as_u64()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn root() -> RootIdentity {
        RootIdentity::from_bytes(b"root-a".to_vec())
    }

    fn source_id() -> SourceId {
        SourceId::from_string("source-a")
    }

    fn checkpoint() -> String {
        serde_json::json!({
            "root_identity": "root-a",
            "event_id": 17,
            "format_version": 3,
            "source_id": "source-a",
            "lifecycle_generation": 4,
            "source_revision": 12,
            "cause": "targeted_replay",
            "continuity_proof": {
                "root_identity": "root-a",
                "backend": "fsevents",
                "backend_device": 99,
                "watcher_generation": 23,
                "replay_coverage_start_event_id": 7,
                "replay_coverage_end_event_id": 17,
                "acknowledged_end_event_id": 17
            }
        })
        .to_string()
    }

    fn read(value: &str) -> Option<ReplayPriorToken> {
        let directory = tempdir().expect("source root");
        let database = SourceDatabase::open_for_test_fixture_source_write(directory.path())
            .expect("source database");
        database
            .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, value)
            .expect("checkpoint metadata");
        database
            .read_durable_replay_prior(&source_id(), &root(), WatcherGeneration::new(4))
            .expect("authority read")
    }

    #[test]
    fn valid_checkpoint_produces_durable_opaque_authority() {
        let token = read(&checkpoint()).expect("valid token");
        assert_eq!(token.source_id(), &source_id());
        assert_eq!(token.root_identity(), &root());
        assert_eq!(token.watcher_generation(), WatcherGeneration::new(4));
        assert_eq!(token.acknowledged_sequence(), 17);
        assert_eq!(
            token.backend_stream_identity().as_bytes(),
            b"fsevents:\0\0\0\0\0\0\0c"
        );
    }

    #[test]
    fn reopened_database_reconstructs_the_same_authority() {
        let directory = tempdir().expect("source root");
        {
            let database = SourceDatabase::open_for_test_fixture_source_write(directory.path())
                .expect("source database");
            database
                .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, &checkpoint())
                .expect("checkpoint metadata");
        }
        let database = SourceDatabase::open_for_test_fixture_source_write(directory.path())
            .expect("reopened source database");
        let token = database
            .read_durable_replay_prior(&source_id(), &root(), WatcherGeneration::new(4))
            .expect("authority read")
            .expect("valid token");
        assert_eq!(token.acknowledged_sequence(), 17);
    }

    #[test]
    fn legacy_v2_proofless_and_invalid_checkpoints_fail_closed() {
        let values = [
            r#"{"root_identity":"root-a","event_id":17}"#,
            r#"{"root_identity":"root-a","event_id":17,"format_version":2,"source_id":"source-a","lifecycle_generation":4,"source_revision":12,"cause":"targeted_replay"}"#,
            r#"{"root_identity":"root-a","event_id":17,"format_version":3,"source_id":"source-a","lifecycle_generation":4,"source_revision":12,"cause":"targeted_replay","continuity_proof":null}"#,
            "not-json",
        ];
        for value in values {
            assert_eq!(read(value), None, "value should fail closed: {value}");
        }
    }

    #[test]
    fn duplicate_unknown_and_mismatched_checkpoint_fields_fail_closed() {
        let cases = [
            format!(
                "{}{}",
                checkpoint().trim_end_matches('}'),
                ",\"event_id\":17}"
            ),
            checkpoint().replace(
                "\"cause\":\"targeted_replay\"",
                "\"cause\":\"completed_fallback_audit\"",
            ),
            checkpoint().replace("\"backend\":\"fsevents\"", "\"backend\":\"notify\""),
            checkpoint().replace("\"backend_device\":99", "\"backend_device\":0"),
            checkpoint().replace(
                "\"replay_coverage_start_event_id\":7",
                "\"replay_coverage_start_event_id\":18",
            ),
            checkpoint().replace("\"source-a\"", "\"other-source\""),
            checkpoint().replace("\"root-a\"", "\"other-root\""),
            checkpoint().replace("\"lifecycle_generation\":4", "\"lifecycle_generation\":5"),
            checkpoint().replace(
                "\"continuity_proof\":{",
                "\"continuity_proof\":{\"unknown\":true,",
            ),
            checkpoint().replace(
                "\"continuity_proof\":{",
                "\"continuity_proof\":{\"backend\":\"notify\",",
            ),
        ];
        for value in cases {
            assert_eq!(read(&value), None, "value should fail closed: {value}");
        }
    }
}
