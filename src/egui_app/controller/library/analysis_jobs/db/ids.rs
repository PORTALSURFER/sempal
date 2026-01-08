use std::path::{Path, PathBuf};

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn parse_sample_id(
    sample_id: &str,
) -> Result<(String, PathBuf), String> {
    let (source, path) = sample_id
        .split_once("::")
        .ok_or_else(|| format!("Invalid sample_id: {sample_id}"))?;
    if source.is_empty() {
        return Err(format!("Invalid sample_id: {sample_id}"));
    }
    if path.is_empty() {
        return Err(format!("Invalid sample_id: {sample_id}"));
    }
    Ok((source.to_string(), PathBuf::from(path)))
}

pub(crate) fn build_sample_id(
    source_id: &str,
    relative_path: &Path,
) -> String {
    let rel = relative_path.to_string_lossy().replace('\\', "/");
    format!("{}::{}", source_id, rel)
}
