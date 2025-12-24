#[derive(Clone, Debug)]
pub(in crate::egui_app::controller::analysis_jobs) struct ClaimedJob {
    pub(in crate::egui_app::controller::analysis_jobs) id: i64,
    pub(in crate::egui_app::controller::analysis_jobs) sample_id: String,
    pub(in crate::egui_app::controller::analysis_jobs) content_hash: Option<String>,
    pub(in crate::egui_app::controller::analysis_jobs) job_type: String,
    pub(in crate::egui_app::controller::analysis_jobs) source_root: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub(in crate::egui_app::controller::analysis_jobs) struct SampleMetadata {
    pub(in crate::egui_app::controller::analysis_jobs) sample_id: String,
    pub(in crate::egui_app::controller::analysis_jobs) content_hash: String,
    pub(in crate::egui_app::controller::analysis_jobs) size: u64,
    pub(in crate::egui_app::controller::analysis_jobs) mtime_ns: i64,
}
