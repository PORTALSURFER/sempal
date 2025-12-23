#[derive(Clone, Debug)]
pub(super) struct ClaimedJob {
    pub(super) id: i64,
    pub(super) sample_id: String,
    pub(super) content_hash: Option<String>,
    pub(super) job_type: String,
}

#[derive(Clone, Debug)]
pub(super) struct SampleMetadata {
    pub(super) sample_id: String,
    pub(super) content_hash: String,
    pub(super) size: u64,
    pub(super) mtime_ns: i64,
}
