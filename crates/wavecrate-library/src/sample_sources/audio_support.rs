use std::path::Path;

/// Supported audio extensions for sample sources (lowercase, without dots).
pub const SUPPORTED_AUDIO_EXTENSIONS: [&str; 1] = ["wav"];

/// Audio-container extensions recognized for unsupported-file diagnostics.
///
/// Recognition does not imply decode, playback, or indexing support.
pub const RECOGNIZED_AUDIO_EXTENSIONS: [&str; 10] = [
    "aac", "aif", "aiff", "flac", "m4a", "mp3", "ogg", "opus", "wav", "wma",
];

/// Return true if the path has a supported audio extension.
pub fn is_supported_audio(path: &Path) -> bool {
    if is_apple_double_sidecar(path) {
        return false;
    }
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    SUPPORTED_AUDIO_EXTENSIONS
        .iter()
        .any(|supported| ext.eq_ignore_ascii_case(supported))
}

/// Return true when a path has a recognized audio-container extension.
pub fn is_recognized_audio(path: &Path) -> bool {
    if is_apple_double_sidecar(path) {
        return false;
    }
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    RECOGNIZED_AUDIO_EXTENSIONS
        .iter()
        .any(|recognized| ext.eq_ignore_ascii_case(recognized))
}

/// Return true for macOS AppleDouble sidecars such as `._kick.wav`.
pub fn is_apple_double_sidecar(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("._"))
}

/// Build a SQL WHERE clause that filters for supported audio extensions.
pub fn supported_audio_where_clause() -> String {
    let exts: Vec<String> = SUPPORTED_AUDIO_EXTENSIONS
        .iter()
        .map(|ext| format!("'{}'", ext))
        .collect();
    format!(
        "extension IN ({}) AND path NOT GLOB '._*' AND path NOT GLOB '*/._*'",
        exts.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_audio_is_wav_first_only() {
        assert!(is_supported_audio(Path::new("kick.wav")));
        assert!(is_supported_audio(Path::new("KICK.WAV")));
        assert!(!is_supported_audio(Path::new("._kick.wav")));
        assert!(!is_supported_audio(Path::new("drums/._kick.wav")));

        for unsupported in ["loop.aif", "loop.aiff", "loop.flac", "loop.mp3"] {
            assert!(
                !is_supported_audio(Path::new(unsupported)),
                "{unsupported} should not be treated as supported audio"
            );
        }
    }

    #[test]
    fn recognized_audio_does_not_expand_supported_formats() {
        for unsupported in ["loop.aif", "loop.aiff", "loop.flac", "loop.mp3"] {
            let path = Path::new(unsupported);
            assert!(is_recognized_audio(path));
            assert!(!is_supported_audio(path));
        }
        assert!(!is_recognized_audio(Path::new("notes.txt")));
        assert!(!is_recognized_audio(Path::new("._loop.flac")));
    }

    #[test]
    fn supported_audio_sql_filter_is_wav_only() {
        assert_eq!(
            supported_audio_where_clause(),
            "extension IN ('wav') AND path NOT GLOB '._*' AND path NOT GLOB '*/._*'"
        );
    }

    #[test]
    fn apple_double_sidecars_are_detected_by_basename() {
        assert!(is_apple_double_sidecar(Path::new("._kick.wav")));
        assert!(is_apple_double_sidecar(Path::new("drums/._kick.wav")));
        assert!(!is_apple_double_sidecar(Path::new("drums/kick._wav")));
        assert!(!is_apple_double_sidecar(Path::new("drums/kick.wav")));
    }
}
