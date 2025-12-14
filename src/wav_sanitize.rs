//! Helpers for tolerating slightly nonstandard WAV headers encountered in the wild.

use std::path::Path;

/// Attempt to sanitize common nonstandard WAV header patterns so strict parsers accept them.
///
/// This is intentionally narrow and conservative: it only rewrites patterns that are commonly
/// accepted by other software but rejected by strict WAV readers.
pub fn sanitize_wav_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() < 12 {
        return bytes;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return bytes;
    }

    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let chunk_data = offset + 8;
        if chunk_data + chunk_size > bytes.len() {
            break;
        }

        if id == b"fmt " {
            if let Some(repaired) =
                shrink_pcm_fmt_chunk_with_padding(&mut bytes, offset, chunk_size)
            {
                return repaired;
            }
            break;
        }

        offset = chunk_data + chunk_size;
        if chunk_size % 2 == 1 {
            offset = offset.saturating_add(1);
        }
    }

    bytes
}

/// Read a file and return sanitized bytes.
pub fn read_sanitized_wav_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let bytes =
        std::fs::read(path).map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    Ok(sanitize_wav_bytes(bytes))
}

fn shrink_pcm_fmt_chunk_with_padding(
    bytes: &mut Vec<u8>,
    chunk_offset: usize,
    chunk_size: usize,
) -> Option<Vec<u8>> {
    if chunk_size <= 18 || chunk_size % 2 != 0 {
        return None;
    }
    let fmt_data = chunk_offset + 8;
    if fmt_data + chunk_size > bytes.len() {
        return None;
    }

    let format_tag = u16::from_le_bytes(bytes[fmt_data..fmt_data + 2].try_into().ok()?);
    // Only apply to PCM (1) or IEEE float (3) where 16 or 18 byte fmt is standard.
    if !matches!(format_tag, 1 | 3) {
        return None;
    }
    // Require the WaveFormatEx "cbSize" field to exist and be 0.
    let cb_size = u16::from_le_bytes(bytes.get(fmt_data + 16..fmt_data + 18)?.try_into().ok()?);
    if cb_size != 0 {
        return None;
    }
    // Only shrink when any extra bytes are all padding zeros.
    if !bytes[fmt_data + 18..fmt_data + chunk_size]
        .iter()
        .all(|b| *b == 0)
    {
        return None;
    }

    // Shrink fmt chunk down to 18 bytes (WaveFormatEx with cbSize=0).
    bytes[chunk_offset + 4..chunk_offset + 8].copy_from_slice(&(18u32).to_le_bytes());
    bytes.drain(fmt_data + 18..fmt_data + chunk_size);

    // Update RIFF size (file size - 8).
    if bytes.len() >= 8 {
        let riff_size = (bytes.len().saturating_sub(8) as u32).to_le_bytes();
        bytes[4..8].copy_from_slice(&riff_size);
    }

    Some(std::mem::take(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::SampleFormat;
    use std::io::Cursor;

    fn wav_bytes_pcm_16bit(samples: &[i16]) -> Vec<u8> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
            for &s in samples {
                writer.write_sample(s).unwrap();
                writer.write_sample(s).unwrap();
            }
            writer.finalize().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn fixes_pcm_fmt_chunk_size_20() {
        let base = wav_bytes_pcm_16bit(&[0, 1000, -1000, 0]);
        assert!(hound::WavReader::new(Cursor::new(base.as_slice())).is_ok());

        // Inflate fmt chunk size to 20 and insert 4 zero bytes after the 16-byte fmt body.
        let mut bad = base.clone();
        assert_eq!(&bad[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bad[16..20].try_into().unwrap()), 16);
        bad[16..20].copy_from_slice(&20u32.to_le_bytes());
        bad.splice(12 + 8 + 16..12 + 8 + 16, [0u8; 4]);
        let riff_len = bad.len();
        bad[4..8].copy_from_slice(&((riff_len - 8) as u32).to_le_bytes());

        assert!(hound::WavReader::new(Cursor::new(bad.as_slice())).is_err());
        let fixed = sanitize_wav_bytes(bad);
        assert!(hound::WavReader::new(Cursor::new(fixed.as_slice())).is_ok());
    }

    #[test]
    fn fixes_pcm_fmt_chunk_size_22_with_padding() {
        let base = wav_bytes_pcm_16bit(&[0, 1000, -1000, 0]);
        assert!(hound::WavReader::new(Cursor::new(base.as_slice())).is_ok());

        // Inflate fmt chunk size to 22 and insert 6 zero bytes after the 16-byte fmt body:
        // 2 bytes for cbSize=0 plus 4 bytes of padding.
        let mut bad = base.clone();
        assert_eq!(&bad[12..16], b"fmt ");
        assert_eq!(u32::from_le_bytes(bad[16..20].try_into().unwrap()), 16);
        bad[16..20].copy_from_slice(&22u32.to_le_bytes());
        bad.splice(12 + 8 + 16..12 + 8 + 16, [0u8; 6]);
        let riff_len = bad.len();
        bad[4..8].copy_from_slice(&((riff_len - 8) as u32).to_le_bytes());

        assert!(hound::WavReader::new(Cursor::new(bad.as_slice())).is_err());
        let fixed = sanitize_wav_bytes(bad);
        assert!(hound::WavReader::new(Cursor::new(fixed.as_slice())).is_ok());
    }
}
