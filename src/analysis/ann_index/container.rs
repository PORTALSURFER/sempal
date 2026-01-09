//! Single-file container for ANN index data.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const ANN_CONTAINER_MAGIC: &[u8; 8] = b"SANNIDX1";
const ANN_CONTAINER_VERSION: u32 = 1;
const ANN_CONTAINER_CHECKSUM_LEN: usize = 32;
const ANN_CONTAINER_HEADER_LEN: usize = 8 + 4 + 4 + 4 + 4 + (8 * 6) + ANN_CONTAINER_CHECKSUM_LEN;
const MAX_MODEL_ID_LEN: u32 = 16 * 1024;

#[derive(Clone, Copy, Debug)]
struct AnnContainerHeader {
    model_id_len: u32,
    graph_offset: u64,
    graph_len: u64,
    data_offset: u64,
    data_len: u64,
    id_map_offset: u64,
    id_map_len: u64,
    checksum: [u8; ANN_CONTAINER_CHECKSUM_LEN],
}

/// Payload extracted from an ANN container.
pub(crate) struct AnnContainerUnpack {
    pub(crate) model_id: String,
    pub(crate) id_map: Vec<String>,
}

/// Write a single-file ANN container from HNSW dumps and id map data.
pub(crate) fn write_container(
    path: &Path,
    model_id: &str,
    graph_path: &Path,
    data_path: &Path,
    id_map: &[String],
) -> Result<(), String> {
    let graph_len = file_len(graph_path)?;
    let data_len = file_len(data_path)?;
    let id_map_bytes = encode_id_map(id_map)?;
    let model_id_bytes = model_id.as_bytes();
    if model_id_bytes.len() > MAX_MODEL_ID_LEN as usize {
        return Err(format!(
            "ANN model id too long: {} bytes (max {})",
            model_id_bytes.len(),
            MAX_MODEL_ID_LEN
        ));
    }
    let header = AnnContainerHeader::new(
        model_id_bytes.len(),
        graph_len,
        data_len,
        id_map_bytes.len(),
    );
    let mut temp = create_tempfile(path)?;
    let file = temp.as_file_mut();
    write_placeholder_header(file)?;
    let mut hasher = Sha256::new();
    write_model_id(file, model_id_bytes, &mut hasher)?;
    copy_with_hash(graph_path, file, &mut hasher)?;
    copy_with_hash(data_path, file, &mut hasher)?;
    write_id_map(file, &id_map_bytes, &mut hasher)?;
    let mut final_header = header;
    final_header.checksum = hasher.finalize().into();
    write_header_at_start(file, final_header)?;
    temp.persist(path)
        .map_err(|err| format!("Failed to persist ANN container: {err}"))?;
    Ok(())
}

/// Unpack an ANN container into HNSW dump files, returning the id map.
pub(crate) fn unpack_container(
    path: &Path,
    output_dir: &Path,
    basename: &str,
) -> Result<AnnContainerUnpack, String> {
    let mut file = open_container(path)?;
    let file_len = file_len_from_file(&file)?;
    let header = AnnContainerHeader::read(&mut file)?;
    header.validate(file_len)?;
    let mut hasher = Sha256::new();
    let model_id = read_model_id(&mut file, &header, &mut hasher)?;
    let id_map_bytes = unpack_payload(&mut file, &header, output_dir, basename, &mut hasher)?;
    verify_checksum(&header, hasher)?;
    let id_map = decode_id_map(&id_map_bytes)?;
    Ok(AnnContainerUnpack { model_id, id_map })
}

impl AnnContainerHeader {
    fn new(model_id_len: usize, graph_len: u64, data_len: u64, id_map_len: usize) -> Self {
        let model_id_len = model_id_len as u32;
        let graph_offset = (ANN_CONTAINER_HEADER_LEN as u64) + (model_id_len as u64);
        let data_offset = graph_offset + graph_len;
        let id_map_offset = data_offset + data_len;
        AnnContainerHeader {
            model_id_len,
            graph_offset,
            graph_len,
            data_offset,
            data_len,
            id_map_offset,
            id_map_len: id_map_len as u64,
            checksum: [0u8; ANN_CONTAINER_CHECKSUM_LEN],
        }
    }

    fn read(file: &mut File) -> Result<Self, String> {
        let header_len = read_header_prefix(file)?;
        let rest = read_header_rest(file, header_len)?;
        parse_header(rest)
    }

    fn validate(&self, file_len: u64) -> Result<(), String> {
        if self.model_id_len > MAX_MODEL_ID_LEN {
            return Err(format!(
                "ANN container model id length too large: {} bytes (max {})",
                self.model_id_len, MAX_MODEL_ID_LEN
            ));
        }
        let expected_graph_offset = (ANN_CONTAINER_HEADER_LEN as u64)
            .checked_add(self.model_id_len as u64)
            .ok_or_else(|| "ANN container header length overflow".to_string())?;
        if self.graph_offset != expected_graph_offset {
            return Err("ANN container graph offset mismatch".to_string());
        }
        let graph_end = self
            .graph_offset
            .checked_add(self.graph_len)
            .ok_or_else(|| "ANN container graph section overflow".to_string())?;
        if graph_end > self.data_offset {
            return Err("ANN container graph section overlaps data".to_string());
        }
        let data_end = self
            .data_offset
            .checked_add(self.data_len)
            .ok_or_else(|| "ANN container data section overflow".to_string())?;
        if data_end > self.id_map_offset {
            return Err("ANN container data section overlaps id map".to_string());
        }
        let end = self
            .id_map_offset
            .checked_add(self.id_map_len)
            .ok_or_else(|| "ANN container id map section overflow".to_string())?;
        if end > file_len {
            return Err("ANN container payload exceeds file length".to_string());
        }
        Ok(())
    }
}

fn write_placeholder_header(file: &mut File) -> Result<(), String> {
    let zeros = vec![0u8; ANN_CONTAINER_HEADER_LEN];
    file.write_all(&zeros)
        .map_err(|err| format!("Failed to write ANN header placeholder: {err}"))
}

fn write_header_at_start(file: &mut File, header: AnnContainerHeader) -> Result<(), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|err| format!("Failed to seek ANN container: {err}"))?;
    let buf = build_header_bytes(header);
    file.write_all(&buf)
        .map_err(|err| format!("Failed to write ANN header: {err}"))?;
    Ok(())
}

fn write_model_id(file: &mut File, model_id: &[u8], hasher: &mut Sha256) -> Result<(), String> {
    file.write_all(model_id)
        .map_err(|err| format!("Failed to write ANN model id: {err}"))?;
    hasher.update(model_id);
    Ok(())
}

fn read_model_id(
    file: &mut File,
    header: &AnnContainerHeader,
    hasher: &mut Sha256,
) -> Result<String, String> {
    if header.model_id_len > MAX_MODEL_ID_LEN {
        return Err(format!(
            "ANN container model id length too large: {} bytes (max {})",
            header.model_id_len, MAX_MODEL_ID_LEN
        ));
    }
    let mut model_id = vec![0u8; header.model_id_len as usize];
    file.read_exact(&mut model_id)
        .map_err(|err| format!("Failed to read ANN model id: {err}"))?;
    hasher.update(&model_id);
    let model_id =
        String::from_utf8(model_id).map_err(|err| format!("ANN model id invalid UTF-8: {err}"))?;
    Ok(model_id)
}

fn open_container(path: &Path) -> Result<File, String> {
    File::open(path).map_err(|err| format!("Failed to open ANN container: {err}"))
}

fn copy_with_hash(src: &Path, out: &mut File, hasher: &mut Sha256) -> Result<(), String> {
    let mut input = File::open(src).map_err(|err| format!("Failed to open ANN data: {err}"))?;
    copy_reader_with_hash(&mut input, out, hasher)
}

fn copy_range_with_hash(
    file: &mut File,
    offset: u64,
    len: u64,
    out_path: &Path,
    hasher: &mut Sha256,
) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("Failed to seek ANN container: {err}"))?;
    let mut reader = file.take(len);
    let mut out =
        File::create(out_path).map_err(|err| format!("Failed to write ANN payload: {err}"))?;
    copy_reader_with_hash(&mut reader, &mut out, hasher)
}

fn copy_reader_with_hash(
    reader: &mut dyn Read,
    out: &mut File,
    hasher: &mut Sha256,
) -> Result<(), String> {
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| format!("Failed to read ANN payload: {err}"))?;
        if read == 0 {
            break;
        }
        out.write_all(&buffer[..read])
            .map_err(|err| format!("Failed to write ANN payload: {err}"))?;
        hasher.update(&buffer[..read]);
    }
    Ok(())
}

fn read_range(file: &mut File, offset: u64, len: u64) -> Result<Vec<u8>, String> {
    let len = usize::try_from(len)
        .map_err(|_| "ANN container payload too large to read".to_string())?;
    let mut buf = vec![0u8; len];
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| format!("Failed to seek ANN container: {err}"))?;
    file.read_exact(&mut buf)
        .map_err(|err| format!("Failed to read ANN payload: {err}"))?;
    Ok(buf)
}

fn encode_id_map(id_map: &[String]) -> Result<Vec<u8>, String> {
    serde_json::to_vec(id_map).map_err(|err| format!("Failed to encode ANN id map: {err}"))
}

fn decode_id_map(bytes: &[u8]) -> Result<Vec<String>, String> {
    serde_json::from_slice(bytes).map_err(|err| format!("Failed to decode ANN id map: {err}"))
}

fn write_id_map(file: &mut File, bytes: &[u8], hasher: &mut Sha256) -> Result<(), String> {
    file.write_all(bytes)
        .map_err(|err| format!("Failed to write ANN id map: {err}"))?;
    hasher.update(bytes);
    Ok(())
}

fn read_u32(reader: &mut dyn Read) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|err| format!("Failed to read ANN header: {err}"))?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(reader: &mut dyn Read) -> Result<u64, String> {
    let mut buf = [0u8; 8];
    reader
        .read_exact(&mut buf)
        .map_err(|err| format!("Failed to read ANN header: {err}"))?;
    Ok(u64::from_le_bytes(buf))
}

fn file_len(path: &Path) -> Result<u64, String> {
    let meta = std::fs::metadata(path)
        .map_err(|err| format!("Failed to read ANN payload metadata: {err}"))?;
    Ok(meta.len())
}

fn file_len_from_file(file: &File) -> Result<u64, String> {
    file.metadata()
        .map(|meta| meta.len())
        .map_err(|err| format!("Failed to read ANN container metadata: {err}"))
}

fn create_tempfile(path: &Path) -> Result<tempfile::NamedTempFile, String> {
    let dir = path
        .parent()
        .ok_or_else(|| "ANN container path missing parent".to_string())?;
    tempfile::Builder::new()
        .prefix("ann_container")
        .tempfile_in(dir)
        .map_err(|err| format!("Failed to create ANN tempfile: {err}"))
}

fn dump_paths_for(
    dir: &Path,
    basename: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let graph = dir.join(format!("{basename}.hnsw.graph"));
    let data = dir.join(format!("{basename}.hnsw.data"));
    Ok((graph, data))
}

fn build_header_bytes(header: AnnContainerHeader) -> Vec<u8> {
    let mut buf = Vec::with_capacity(ANN_CONTAINER_HEADER_LEN);
    buf.extend_from_slice(ANN_CONTAINER_MAGIC);
    buf.extend_from_slice(&ANN_CONTAINER_VERSION.to_le_bytes());
    buf.extend_from_slice(&(ANN_CONTAINER_HEADER_LEN as u32).to_le_bytes());
    buf.extend_from_slice(&header.model_id_len.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&header.graph_offset.to_le_bytes());
    buf.extend_from_slice(&header.graph_len.to_le_bytes());
    buf.extend_from_slice(&header.data_offset.to_le_bytes());
    buf.extend_from_slice(&header.data_len.to_le_bytes());
    buf.extend_from_slice(&header.id_map_offset.to_le_bytes());
    buf.extend_from_slice(&header.id_map_len.to_le_bytes());
    buf.extend_from_slice(&header.checksum);
    buf
}

fn unpack_payload(
    file: &mut File,
    header: &AnnContainerHeader,
    output_dir: &Path,
    basename: &str,
    hasher: &mut Sha256,
) -> Result<Vec<u8>, String> {
    let (graph_path, data_path) = dump_paths_for(output_dir, basename)?;
    copy_range_with_hash(
        file,
        header.graph_offset,
        header.graph_len,
        &graph_path,
        hasher,
    )?;
    copy_range_with_hash(
        file,
        header.data_offset,
        header.data_len,
        &data_path,
        hasher,
    )?;
    let id_map_bytes = read_range(file, header.id_map_offset, header.id_map_len)?;
    hasher.update(&id_map_bytes);
    Ok(id_map_bytes)
}

fn verify_checksum(header: &AnnContainerHeader, hasher: Sha256) -> Result<(), String> {
    let checksum: [u8; ANN_CONTAINER_CHECKSUM_LEN] = hasher.finalize().into();
    if checksum != header.checksum {
        return Err("ANN container checksum mismatch".to_string());
    }
    Ok(())
}

fn read_header_prefix(file: &mut File) -> Result<usize, String> {
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix)
        .map_err(|err| format!("Failed to read ANN container header: {err}"))?;
    if &prefix[..8] != ANN_CONTAINER_MAGIC {
        return Err("ANN container magic mismatch".to_string());
    }
    let version = u32::from_le_bytes(prefix[8..12].try_into().unwrap());
    if version != ANN_CONTAINER_VERSION {
        return Err(format!("ANN container version mismatch: {version}"));
    }
    let header_len = u32::from_le_bytes(prefix[12..16].try_into().unwrap()) as usize;
    if header_len != ANN_CONTAINER_HEADER_LEN {
        return Err(format!(
            "ANN container header length mismatch: {header_len}"
        ));
    }
    Ok(header_len)
}

fn read_header_rest(file: &mut File, header_len: usize) -> Result<Vec<u8>, String> {
    let mut rest = vec![0u8; header_len - 16];
    file.read_exact(&mut rest)
        .map_err(|err| format!("Failed to read ANN container header: {err}"))?;
    Ok(rest)
}

fn parse_header(rest: Vec<u8>) -> Result<AnnContainerHeader, String> {
    let mut cursor = std::io::Cursor::new(rest);
    let model_id_len = read_u32(&mut cursor)?;
    let _reserved = read_u32(&mut cursor)?;
    let graph_offset = read_u64(&mut cursor)?;
    let graph_len = read_u64(&mut cursor)?;
    let data_offset = read_u64(&mut cursor)?;
    let data_len = read_u64(&mut cursor)?;
    let id_map_offset = read_u64(&mut cursor)?;
    let id_map_len = read_u64(&mut cursor)?;
    let mut checksum = [0u8; ANN_CONTAINER_CHECKSUM_LEN];
    cursor
        .read_exact(&mut checksum)
        .map_err(|err| format!("Failed to read ANN checksum: {err}"))?;
    Ok(AnnContainerHeader {
        model_id_len,
        graph_offset,
        graph_len,
        data_offset,
        data_len,
        id_map_offset,
        id_map_len,
        checksum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ann_container_rejects_oversized_model_id_len() {
        let model_id_len = MAX_MODEL_ID_LEN + 1;
        let graph_offset =
            (ANN_CONTAINER_HEADER_LEN as u64).checked_add(model_id_len as u64).unwrap();
        let header = AnnContainerHeader {
            model_id_len,
            graph_offset,
            graph_len: 0,
            data_offset: graph_offset,
            data_len: 0,
            id_map_offset: graph_offset,
            id_map_len: 0,
            checksum: [0u8; ANN_CONTAINER_CHECKSUM_LEN],
        };
        let err = header.validate(graph_offset).unwrap_err();
        assert!(err.contains("model id length"));
    }

    #[test]
    fn ann_container_rejects_offset_overflow() {
        let header = AnnContainerHeader {
            model_id_len: 0,
            graph_offset: ANN_CONTAINER_HEADER_LEN as u64,
            graph_len: 0,
            data_offset: ANN_CONTAINER_HEADER_LEN as u64,
            data_len: 0,
            id_map_offset: u64::MAX - 1,
            id_map_len: 10,
            checksum: [0u8; ANN_CONTAINER_CHECKSUM_LEN],
        };
        let err = header.validate(u64::MAX).unwrap_err();
        assert!(err.contains("id map section overflow"));
    }

    #[test]
    fn ann_container_rejects_inconsistent_graph_offset() {
        let header = AnnContainerHeader {
            model_id_len: 4,
            graph_offset: ANN_CONTAINER_HEADER_LEN as u64 + 1,
            graph_len: 0,
            data_offset: ANN_CONTAINER_HEADER_LEN as u64 + 1,
            data_len: 0,
            id_map_offset: ANN_CONTAINER_HEADER_LEN as u64 + 1,
            id_map_len: 0,
            checksum: [0u8; ANN_CONTAINER_CHECKSUM_LEN],
        };
        let err = header.validate(u64::MAX).unwrap_err();
        assert!(err.contains("graph offset mismatch"));
    }
}
