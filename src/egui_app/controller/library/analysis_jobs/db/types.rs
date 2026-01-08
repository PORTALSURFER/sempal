#[derive(Clone, Debug)]
pub(crate) struct ClaimedJob {
    pub(crate) id: i64,
    pub(crate) sample_id: String,
    pub(crate) content_hash: Option<String>,
    pub(crate) job_type: String,
    pub(crate) source_root: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct SampleMetadata {
    pub(crate) sample_id: String,
    pub(crate) content_hash: String,
    pub(crate) size: u64,
    pub(crate) mtime_ns: i64,
}
