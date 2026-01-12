use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

/// A reader that transparently sanitizes WAV headers on the fly.
///
/// It reads the first few KB of the file into memory to check for and fix
/// common header issues (like malformed fmt chunks). If a fix is applied,
/// it serves the fixed header from memory and then chains the rest of the
/// file from disk. If no fix is needed, it acts as a pass-through to the file.
pub enum SanitizedWavReader {
    /// Read directly from the file without modification.
    PassThrough(File),
    /// Read a sanitized header buffer followed by the original file body.
    Chained {
        /// The sanitized header data.
        header: Cursor<Vec<u8>>,
        /// The underlying file.
        file: File,
        /// The logical size of the header (how many bytes usually come from `header`).
        header_len: u64,
        /// The physical offset in the file where the body begins (i.e. where `header` ends).
        file_start_offset: u64,
    },
}

impl Read for SanitizedWavReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            SanitizedWavReader::PassThrough(file) => file.read(buf),
            SanitizedWavReader::Chained {
                header,
                file,
                header_len: _,
                file_start_offset: _,
            } => {
                // First read from the header buffer.
                let n = header.read(buf)?;
                if n > 0 {
                    return Ok(n);
                }
                // If header is exhausted, read from the file.
                file.read(buf)
            }
        }
    }
}

impl Seek for SanitizedWavReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SanitizedWavReader::PassThrough(file) => file.seek(pos),
            SanitizedWavReader::Chained {
                header,
                file,
                header_len,
                file_start_offset,
            } => {
                match pos {
                    SeekFrom::Start(offset) => {
                        if offset < *header_len {
                            header.set_position(offset);
                            // Sync file position just in case, though technically not needed until we read past header.
                            // But usually we just let the file stay where it is until we need it.
                            Ok(offset)
                        } else {
                            // Seeking into the file body.
                            // Logically: offset
                            // Physically: file_start_offset + (offset - header_len)
                            header.set_position(*header_len); // Mark header as done
                            let physical_offset = *file_start_offset + (offset - *header_len);
                            file.seek(SeekFrom::Start(physical_offset)).map(|_| offset)
                        }
                    }
                    SeekFrom::Current(delta) => {
                        let current_logical = if header.position() < *header_len {
                            header.position()
                        } else {
                            // If header is exhausted, logical pos is header_len + (file_phys_pos - file_start_offset)
                            // But we can simplify: we know our current logical position.
                            // Let's implement Current via Start for simplicity to ensure correctness,
                            // or track global logical position.
                            // Actually, standard approach: get current pos, add delta, seek Start.
                            // But we need to know current pos.
                            *header_len + (file.stream_position()? - *file_start_offset)
                        };

                        let new_pos = if delta >= 0 {
                            current_logical.checked_add(delta as u64)
                        } else {
                            current_logical.checked_sub((-delta) as u64)
                        };
                        match new_pos {
                            Some(p) => self.seek(SeekFrom::Start(p)),
                            None => Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "invalid seek to a negative or overflowed position",
                            )),
                        }
                    }
                    SeekFrom::End(delta) => {
                        // We need the file size.
                        let file_end = file.seek(SeekFrom::End(0))?;
                        // logical_len = header_len + (file_len - file_start_offset)
                        let logical_len = *header_len + (file_end - *file_start_offset);
                        let new_pos = if delta >= 0 {
                            logical_len.checked_add(delta as u64)
                        } else {
                            logical_len.checked_sub((-delta) as u64)
                        };
                         match new_pos {
                            Some(p) => self.seek(SeekFrom::Start(p)),
                            None => Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "invalid seek to a negative or overflowed position",
                            )),
                        }
                    }
                }
            }
        }
    }
}

/// Open a WAV file with transparent header sanitization.
pub fn open_sanitized_wav(path: &Path) -> Result<SanitizedWavReader, String> {
    let mut file =
        File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;

    // Read the first 4KB to check/fix header.
    let mut buffer = Vec::with_capacity(4096);
    // Use by_ref() to borrow file mutably so we don't consume it
    let mut chunk = file.by_ref().take(4096);
    chunk
        .read_to_end(&mut buffer)
        .map_err(|err| format!("Failed to read header of {}: {err}", path.display()))?;
    
    // We need the restore the file handle (take consumes it, but we can recover it or just clone it before? 
    // `take` takes `&mut file`. `chunk` borrows `file`. `chunk.read_to_end` works.
    // `chunk` drops, `file` is available again.
    // However, the file position is now advanced by `buffer.len()`.

    let read_len = buffer.len();
    if read_len == 0 {
         // Empty file?
         return Ok(SanitizedWavReader::PassThrough(file));
    }

    // Get total file size to pass to sanitizer (needed for RIFF size fix)
    let total_size = file.metadata()
        .map(|m| m.len())
        .unwrap_or(read_len as u64); // Fallback if metadata fails?

    if sanitize_wav_header(&mut buffer, total_size) {
        // Fix applied.
        let header_len = buffer.len() as u64;
        let file_start_offset = read_len as u64;
        
        // Ensure file is positioned correctly for the body.
        // It should be at `read_len` already because of `read_to_end`.
        
        Ok(SanitizedWavReader::Chained {
            header: Cursor::new(buffer),
            file,
            header_len,
            file_start_offset,
        })
    } else {
        // No fix needed. Rewind and return plain file.
        file.seek(SeekFrom::Start(0))
            .map_err(|err| format!("Failed to rewind {}: {err}", path.display()))?;
        Ok(SanitizedWavReader::PassThrough(file))
    }
}


/// Refactored from `sanitize_wav_bytes`: inspects `bytes` (the file header) and modifies it in-place if needed.
/// Returns true if changes were made.
fn sanitize_wav_header(bytes: &mut Vec<u8>, total_file_len: u64) -> bool {
    if bytes.len() < 12 {
        return false;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return false;
    }

    let mut offset = 12usize;

    // We only iterate as long as we are within the buffer.
    // If a chunk extends beyond the buffer, we stop scanning (can't fix what we don't see).
    while offset + 8 <= bytes.len() {
        let id_slice = &bytes[offset..offset + 4];
        let id = [id_slice[0], id_slice[1], id_slice[2], id_slice[3]]; // copy to array for comparison
        
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let chunk_data = offset + 8;
        
        // If chunk data extends beyond our buffer, we can't safely inspect/fix it 
        // if it relies on content access.
        if chunk_data + chunk_size > bytes.len() {
            // Special case: if it IS the fmt chunk and we have enough bytes to see the crucial parts
            // maybe we can still fix it? 
            // `shrink_pcm_fmt_chunk_with_padding` requires `chunk_size` bytes to be available to check padding.
            // So if it's cut off, we can't fix it.
            break;
        }

        if &id == b"fmt " {
            if shrink_pcm_fmt_chunk_with_padding(bytes, offset, chunk_size, total_file_len) {
                return true;
            }
            // If fmt found but not fixed, we stop looking for fmt.
            break;
        }

        offset = chunk_data + chunk_size;
        if chunk_size % 2 == 1 {
            offset = offset.saturating_add(1);
        }
    }

    false
}

// Deprecate or keep for compatibility? The user asked to "Change `sanitize_wav_bytes` to accept a `Read` or `Seek` trait".
// But `open_sanitized_wav` handles the file I/O safely.
// We can keep `read_sanitized_wav_bytes` for existing tests but make it use the new logic or just reimplement
// it using `sanitize_wav_header` (which is what I did basically, just extracted the logic).
// I'll keep `read_sanitized_wav_bytes` for now to avoid breaking other potential usages not found by grep,
// or just as a utility.
/// Read a WAV file, sanitizing the header in-memory if required.
pub fn read_sanitized_wav_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes =
        std::fs::read(path).map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    let len = bytes.len() as u64;
    sanitize_wav_header(&mut bytes, len);
    Ok(bytes)
}

/// Helper: Attempts to shrink an over-padded PCM fmt chunk.
/// Returns true if modified.
fn shrink_pcm_fmt_chunk_with_padding(
    bytes: &mut Vec<u8>,
    chunk_offset: usize,
    chunk_size: usize,
    total_file_len: u64,
) -> bool {
    if chunk_size <= 18 || chunk_size % 2 != 0 {
        return false;
    }
    let fmt_data = chunk_offset + 8;
    if fmt_data + chunk_size > bytes.len() {
        return false;
    }

    let format_tag = match bytes[fmt_data..fmt_data + 2].try_into() {
        Ok(b) => u16::from_le_bytes(b),
        Err(_) => return false,
    };
    
    // Only apply to PCM (1) or IEEE float (3) where 16 or 18 byte fmt is standard.
    if !matches!(format_tag, 1 | 3) {
        return false;
    }
    // Require the WaveFormatEx "cbSize" field to exist and be 0.
    // cbSize is at offset 16 relative to fmt_data (i.e. bytes 16..18 of chunk data)
    let cb_size_offset = fmt_data + 16;
    if cb_size_offset + 2 > bytes.len() { return false; }
    
    let cb_size = u16::from_le_bytes(bytes[cb_size_offset..cb_size_offset + 2].try_into().unwrap());
    if cb_size != 0 {
        return false;
    }
    
    // Only shrink when any extra bytes are all padding zeros.
    // data starts at fmt_data. 18 bytes are: 16 bytes basic + 2 bytes cbSize.
    // Check bytes from fmt_data + 18 up to fmt_data + chunk_size.
    if !bytes[fmt_data + 18..fmt_data + chunk_size]
        .iter()
        .all(|b| *b == 0)
    {
        return false;
    }

    // Shrink fmt chunk down to 18 bytes (WaveFormatEx with cbSize=0).
    // 1. Update chunk size in header (offset+4..offset+8)
    bytes[chunk_offset + 4..chunk_offset + 8].copy_from_slice(&(18u32).to_le_bytes());
    
    // 2. Remove the extra padding bytes.
    // Range to remove: from fmt_data + 18 to fmt_data + chunk_size
    bytes.drain(fmt_data + 18..fmt_data + chunk_size);

    // 3. Update RIFF size (total_file_len - 8).
    // Note: total_file_len was the ORIGINAL size.
    // We removed (chunk_size - 18) bytes.
    // New size = total_file_len - (chunk_size - 18).
    // RIFF size = New size - 8.
    
    let removed_count = chunk_size - 18;
    let new_len = total_file_len.saturating_sub(removed_count as u64);
    
    if bytes.len() >= 8 {
        let riff_size = (new_len.saturating_sub(8) as u32).to_le_bytes();
        bytes[4..8].copy_from_slice(&riff_size);
    }

    true
}

// Kept for backward compat in signatures primarily, but renamed/modified above.
// Actually the original code had `sanitize_wav_bytes(mut bytes: Vec<u8>) -> Vec<u8>`.
// I'll add a wrapper to keep the signature compatible for tests
/// Sanitize a WAV header in-memory and return the updated bytes.
pub fn sanitize_wav_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
    let len = bytes.len() as u64;
    sanitize_wav_header(&mut bytes, len);
    bytes
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
        // Inflate fmt chunk size to 20 and insert 4 zero bytes after the 16-byte fmt body.
        let mut bad = base.clone();
        bad[16..20].copy_from_slice(&20u32.to_le_bytes());
        bad.splice(12 + 8 + 16..12 + 8 + 16, [0u8; 4]);
        let riff_len = bad.len();
        bad[4..8].copy_from_slice(&((riff_len - 8) as u32).to_le_bytes());

        // Use the new signature wrapper
        let fixed = sanitize_wav_bytes(bad);
        assert!(hound::WavReader::new(Cursor::new(fixed.as_slice())).is_ok());
    }

    #[test]
    fn fixes_pcm_fmt_chunk_size_22_with_padding() {
        let base = wav_bytes_pcm_16bit(&[0, 1000, -1000, 0]);
        let mut bad = base.clone();
        bad[16..20].copy_from_slice(&22u32.to_le_bytes());
        bad.splice(12 + 8 + 16..12 + 8 + 16, [0u8; 6]);
        let riff_len = bad.len();
        bad[4..8].copy_from_slice(&((riff_len - 8) as u32).to_le_bytes());

        let fixed = sanitize_wav_bytes(bad);
        assert!(hound::WavReader::new(Cursor::new(fixed.as_slice())).is_ok());
    }

    #[test]
    fn test_open_sanitized_wav_chained() {
        use std::io::Read;
        fn wav_bytes_pcm_16bit(samples: &[i16]) -> Vec<u8> {
             // Redefine or use from outer scope? 
             // The outer `wav_bytes_pcm_16bit` is available in the module.
             super::tests::wav_bytes_pcm_16bit(samples)
        }
        // Wait, I am inside `mod tests`, so `wav_bytes_pcm_16bit` is a sibling.
        let base = wav_bytes_pcm_16bit(&[0, 1000, -1000, 0]);
        let mut bad = base.clone();
        
        // Malform it: fmt chunk size 20, 4 bytes padding
        bad[16..20].copy_from_slice(&20u32.to_le_bytes());
        bad.splice(12 + 8 + 16..12 + 8 + 16, [0u8; 4]);
        let riff_len = bad.len();
        bad[4..8].copy_from_slice(&((riff_len - 8) as u32).to_le_bytes());

        let dir = std::env::temp_dir();
        let path = dir.join("test_open_sanitized.wav");
        std::fs::write(&path, &bad).unwrap();

        let mut reader = open_sanitized_wav(&path).unwrap();
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).unwrap();

        // The read buffer should be the FIXED version
        assert_ne!(buf, bad); 
        // Logic: The fixed version should be header-shrunk.
        // `bad` has 20 byte fmt + 4 padding = 24 bytes data + 8 bytes header = 32 bytes chunk. 
        // Fixed has 18 byte fmt + 0 padding = 18 bytes data + 8 bytes header = 26 bytes chunk.
        // Difference = 6 bytes.
        assert_eq!(buf.len(), bad.len() - 2);
        
        // Validating with hound
        assert!(hound::WavReader::new(Cursor::new(&buf)).is_ok());

        // Cleanup
        let _ = std::fs::remove_file(path);
    }
}
