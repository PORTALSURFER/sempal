use std::path::Path;

/// Supported audio extensions for sample sources (lowercase, without dots).
pub(crate) const SUPPORTED_AUDIO_EXTENSIONS: [&str; 5] = ["wav", "aif", "aiff", "flac", "mp3"];

/// Return true if the path has a supported audio extension.
pub(crate) fn is_supported_audio(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    SUPPORTED_AUDIO_EXTENSIONS
        .iter()
        .any(|supported| ext.eq_ignore_ascii_case(supported))
}

/// Build a SQL WHERE clause that filters for supported audio extensions.
pub(crate) fn supported_audio_where_clause() -> String {
    let exts: Vec<String> = SUPPORTED_AUDIO_EXTENSIONS
        .iter()
        .map(|ext| format!("'{}'", ext))
        .collect();
    format!("extension IN ({})", exts.join(", "))
}
