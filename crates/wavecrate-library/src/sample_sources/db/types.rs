use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Rating applied to a wav file to mark keep/trash decisions.
/// Positive values (1..=3) are Keep.
/// Negative values (-3..=-1) are Trash.
/// 0 is Neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Rating(i8);

impl Rating {
    /// Neutral rating (no keep/trash decision).
    pub const NEUTRAL: Self = Self(0);
    /// Keep rating at level 1.
    pub const KEEP_1: Self = Self(1);
    /// Keep rating at level 3.
    pub const KEEP_3: Self = Self(3);
    /// Trash rating at level 1.
    pub const TRASH_1: Self = Self(-1);
    /// Trash rating at level 3.
    pub const TRASH_3: Self = Self(-3);

    /// Clamp a raw rating into the supported range.
    pub fn new(val: i8) -> Self {
        Self(val.clamp(-3, 3))
    }

    /// Return the underlying rating value.
    pub fn val(&self) -> i8 {
        self.0
    }

    /// Return true when the rating is neutral.
    pub fn is_neutral(&self) -> bool {
        self.0 == 0
    }

    /// Return true when the rating indicates keep.
    pub fn is_keep(&self) -> bool {
        self.0 > 0
    }

    /// Return true when the rating indicates trash.
    pub fn is_trash(&self) -> bool {
        self.0 < 0
    }

    /// Convert the tag to a SQLite-friendly integer.
    pub fn as_i64(self) -> i64 {
        self.0 as i64
    }

    /// Parse an integer column value into a tag.
    /// Values are clamped into the supported range to keep persisted tags stable.
    pub fn from_i64(value: i64) -> Self {
        Self(value.clamp(-3, 3) as i8)
    }
}

/// Fixed sample collection slot assigned to one wav file.
///
/// Slots are zero-based internally (`0..=5`) and displayed as `1..6` in the
/// GUI so the value maps directly to the number-row hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleCollection(u8);

impl SampleCollection {
    /// Number of fixed collection slots exposed by the UI.
    pub const COUNT: usize = 6;

    /// Build a collection slot from a zero-based index.
    pub fn new(index: u8) -> Option<Self> {
        (index < Self::COUNT as u8).then_some(Self(index))
    }

    /// Return the zero-based collection index.
    pub fn index(self) -> u8 {
        self.0
    }

    /// Convert the slot to a SQLite-friendly integer.
    pub fn as_i64(self) -> i64 {
        self.0 as i64
    }

    /// Parse a SQLite integer into a valid slot.
    pub fn from_i64(value: i64) -> Option<Self> {
        u8::try_from(value).ok().and_then(Self::new)
    }
}

/// Canonical sound classifications stored for browser auto-rename metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleSoundType {
    /// Kick drum sample.
    Kick,
    /// Snare drum sample.
    Snare,
    /// Clap sample.
    Clap,
    /// Closed or open hat sample.
    Hat,
    /// Generic percussion sample.
    Perc,
    /// Tom drum sample.
    Tom,
    /// Rimshot sample.
    Rim,
    /// Bass sample.
    Bass,
    /// Sub-bass sample.
    Sub,
    /// Chord sample.
    Chord,
    /// Stab sample.
    Stab,
    /// Pad sample.
    Pad,
    /// Lead sample.
    Lead,
    /// Arpeggio sample.
    Arp,
    /// Sequenced phrase sample.
    Seq,
    /// Vocal sample.
    Vocal,
    /// FX sample.
    Fx,
    /// Texture or ambience sample.
    Texture,
}

impl SampleSoundType {
    /// Return the stable filename/database token for this sound classification.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::Clap => "clap",
            Self::Hat => "hat",
            Self::Perc => "perc",
            Self::Tom => "tom",
            Self::Rim => "rim",
            Self::Bass => "bass",
            Self::Sub => "sub",
            Self::Chord => "chord",
            Self::Stab => "stab",
            Self::Pad => "pad",
            Self::Lead => "lead",
            Self::Arp => "arp",
            Self::Seq => "SEQ",
            Self::Vocal => "vocal",
            Self::Fx => "fx",
            Self::Texture => "texture",
        }
    }

    /// Parse one persisted token into the canonical sound classification.
    pub fn from_token(token: &str) -> Option<Self> {
        match token.trim() {
            "kick" => Some(Self::Kick),
            "snare" => Some(Self::Snare),
            "clap" => Some(Self::Clap),
            "hat" => Some(Self::Hat),
            "perc" => Some(Self::Perc),
            "tom" => Some(Self::Tom),
            "rim" => Some(Self::Rim),
            "bass" => Some(Self::Bass),
            "sub" => Some(Self::Sub),
            "chord" => Some(Self::Chord),
            "stab" => Some(Self::Stab),
            "pad" => Some(Self::Pad),
            "lead" => Some(Self::Lead),
            "arp" => Some(Self::Arp),
            "SEQ" | "seq" => Some(Self::Seq),
            "vocal" => Some(Self::Vocal),
            "fx" => Some(Self::Fx),
            "texture" => Some(Self::Texture),
            _ => None,
        }
    }

    /// Best-effort filename inference used when no explicit sound metadata exists yet.
    pub fn infer_from_name(name: &str) -> Option<Self> {
        let normalized = name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>();
        let words = normalized.split_whitespace().collect::<Vec<_>>();
        const SOUND_TYPES: [SampleSoundType; 18] = [
            SampleSoundType::Kick,
            SampleSoundType::Snare,
            SampleSoundType::Clap,
            SampleSoundType::Hat,
            SampleSoundType::Perc,
            SampleSoundType::Tom,
            SampleSoundType::Rim,
            SampleSoundType::Bass,
            SampleSoundType::Sub,
            SampleSoundType::Chord,
            SampleSoundType::Stab,
            SampleSoundType::Pad,
            SampleSoundType::Lead,
            SampleSoundType::Arp,
            SampleSoundType::Seq,
            SampleSoundType::Vocal,
            SampleSoundType::Fx,
            SampleSoundType::Texture,
        ];
        SOUND_TYPES.into_iter().find(|sound_type| {
            let token = sound_type.token().to_ascii_lowercase();
            words.iter().any(|word| *word == token)
        })
    }
}

/// Details about a wav file stored in a source database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WavEntry {
    /// File path relative to the source root.
    pub relative_path: PathBuf,
    /// File size in bytes.
    pub file_size: u64,
    /// Last modified timestamp in epoch nanoseconds.
    pub modified_ns: i64,
    /// Optional content hash for change detection.
    pub content_hash: Option<String>,
    /// Current rating/tag for the file.
    pub tag: Rating,
    /// True when the sample is marked as a loop for quick filtering in the UI.
    #[serde(default)]
    pub looped: bool,
    /// Canonical sound classification used by browser metadata tools.
    #[serde(default)]
    pub sound_type: Option<SampleSoundType>,
    /// True when the sample has been promoted into the top keep state and should render as locked.
    ///
    /// The lock marker survives reloads so repeated keep-confirmation can show up
    /// consistently across browser refreshes, rescans, and app restarts.
    #[serde(default)]
    pub locked: bool,
    /// Whether the file is missing on disk.
    pub missing: bool,
    /// Epoch seconds of the most recent playback, if any.
    #[serde(default)]
    pub last_played_at: Option<i64>,
    /// Epoch seconds of the most recent explicit library curation decision, if any.
    #[serde(default)]
    pub last_curated_at: Option<i64>,
    /// Optional single custom tag authored by the user.
    #[serde(default)]
    pub user_tag: Option<String>,
    /// Normal library tag labels assigned to the sample.
    #[serde(default)]
    pub normal_tags: Vec<String>,
    /// True when the sample filename is known to have been produced from tag metadata.
    #[serde(default)]
    pub tag_named: bool,
}

/// Browser-facing metadata for one tracked audio file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserFileMetadata {
    /// File path relative to the source root.
    pub relative_path: PathBuf,
    /// File size recorded by the authoritative source manifest.
    pub file_size: u64,
    /// Last modified timestamp recorded by the authoritative source manifest.
    pub modified_ns: i64,
    /// Whether the authoritative manifest currently considers the file absent.
    pub missing: bool,
    /// Current rating/tag for the file.
    pub rating: Rating,
    /// Whether the keep rating is locked.
    pub locked: bool,
    /// Fixed collection slots assigned to the file, in stable slot order.
    pub collections: Vec<SampleCollection>,
    /// Epoch seconds of the most recent playback, if any.
    pub last_played_at: Option<i64>,
    /// Epoch seconds of the most recent explicit curation decision, if any.
    pub last_curated_at: Option<i64>,
}

/// Coherent browser metadata projection read from one committed SQLite snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserMetadataSnapshot {
    /// Source metadata revision observed by the snapshot, or zero for legacy databases.
    pub revision: u64,
    /// Metadata rows in deterministic path order.
    pub files: Vec<BrowserFileMetadata>,
}

/// One bounded, keyset-paged browser metadata result at an exact source revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserMetadataPage {
    /// Source metadata revision observed by the page.
    pub revision: u64,
    /// Metadata rows in deterministic path order, including collection memberships.
    pub files: Vec<BrowserFileMetadata>,
    /// Cursor to pass to the next page, or `None` when this is the final page.
    pub next_path: Option<PathBuf>,
}

/// Authoritative identity facts for one live source-manifest row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceManifestEntry {
    /// File path relative to the source root.
    pub relative_path: PathBuf,
    /// Stable filesystem-object identity when the platform provides one.
    pub file_identity: Option<String>,
    /// Full content hash when deep hashing has completed.
    pub content_hash: Option<String>,
    /// File size in bytes.
    pub file_size: u64,
    /// Last modified timestamp in epoch nanoseconds.
    pub modified_ns: i64,
}

/// Durable classification for a file that is intentionally outside the supported sample manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceIndexClassification {
    /// A recognized audio container that Wavecrate does not currently support.
    UnsupportedAudio,
    /// A regular file that is not recognized as audio.
    UnsupportedNonAudio,
    /// A source entry whose type or file facts could not be inspected safely.
    Inaccessible,
    /// Supported-format audio rejected by an explicit practical constraint.
    PracticallyUnsupportedAudio,
}

impl SourceIndexClassification {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::UnsupportedAudio => "unsupported_audio",
            Self::UnsupportedNonAudio => "unsupported_non_audio",
            Self::Inaccessible => "inaccessible",
            Self::PracticallyUnsupportedAudio => "practically_unsupported_audio",
        }
    }

    pub(crate) fn from_token(token: &str) -> Option<Self> {
        match token {
            "unsupported_audio" => Some(Self::UnsupportedAudio),
            "unsupported_non_audio" => Some(Self::UnsupportedNonAudio),
            "inaccessible" => Some(Self::Inaccessible),
            "practically_unsupported_audio" => Some(Self::PracticallyUnsupportedAudio),
            _ => None,
        }
    }
}

/// Bounded diagnostic attached to an index-only entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceIndexDiagnostic {
    /// The filesystem entry type could not be inspected without following links.
    EntryTypeUnavailable,
    /// File size, modification time, or stable identity could not be inspected.
    MetadataUnavailable,
    /// A regular file could not be opened through the source-root boundary.
    OpenUnavailable,
    /// An explicit practical audio constraint rejected the file.
    PracticalSupportLimit,
    /// The file name is not Unicode and cannot become a normal sample row.
    NonUnicodePath,
}

impl SourceIndexDiagnostic {
    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::EntryTypeUnavailable => "entry_type_unavailable",
            Self::MetadataUnavailable => "metadata_unavailable",
            Self::OpenUnavailable => "open_unavailable",
            Self::PracticalSupportLimit => "practical_support_limit",
            Self::NonUnicodePath => "non_unicode_path",
        }
    }

    pub(crate) fn from_token(token: &str) -> Option<Self> {
        match token {
            "entry_type_unavailable" => Some(Self::EntryTypeUnavailable),
            "metadata_unavailable" => Some(Self::MetadataUnavailable),
            "open_unavailable" => Some(Self::OpenUnavailable),
            "practical_support_limit" => Some(Self::PracticalSupportLimit),
            "non_unicode_path" => Some(Self::NonUnicodePath),
            _ => None,
        }
    }
}

/// Restart-safe facts for one source file intentionally excluded from normal sample state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIndexEntry {
    /// File path relative to the configured source root.
    pub relative_path: PathBuf,
    /// Typed reason this path is outside the supported sample manifest.
    pub classification: SourceIndexClassification,
    /// File size when metadata inspection succeeded.
    pub file_size: Option<u64>,
    /// Last modified timestamp in epoch nanoseconds when inspection succeeded.
    pub modified_ns: Option<i64>,
    /// Stable filesystem-object identity when available.
    pub file_identity: Option<String>,
    /// Bounded diagnostic category without host paths or unbounded error text.
    pub diagnostic: Option<SourceIndexDiagnostic>,
    /// Format-classification policy that produced this row.
    pub format_policy_version: u32,
}

/// Coherent read projection of the durable index-only file set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIndexSnapshot {
    /// Monotonic index-only revision, independent from the supported manifest revision.
    pub revision: u64,
    /// Index-only entries in deterministic path order.
    pub entries: Vec<SourceIndexEntry>,
}

/// One normal library tag stored in a source database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTag {
    /// Stable source-local tag row id.
    pub id: i64,
    /// User-facing label preserved for display.
    pub display_label: String,
    /// Canonical identity used to avoid obvious duplicate tags.
    pub normalized_text: String,
}

/// A tag candidate plus persisted assignment usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTagUsage {
    /// Tag metadata.
    pub tag: SourceTag,
    /// Number of wav rows currently assigned to this tag.
    pub assignment_count: u64,
}
