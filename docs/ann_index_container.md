# ANN Container Format

This document describes the single-file ANN index container stored alongside each
source `library.db` file. The container replaces the legacy `ann/` directory
pair of `*.hnsw.graph`/`*.hnsw.data` plus `*.idmap.json`.

## Location

- Current container path: `similarity_hnsw.ann` in the same folder as `library.db`.
- Legacy files (migration source): `<source>/ann/similarity_hnsw.hnsw.*` and
  `<source>/ann/similarity_hnsw.idmap.json`.

## Layout (v1)

All integers are little-endian. The file is a header followed by payload blocks.

- Header (104 bytes)
  - Magic: `SANNIDX1` (8 bytes)
  - Version: `u32` (currently `1`)
  - Header length: `u32` (currently `104`)
  - Model id length: `u32`
  - Reserved: `u32`
  - Graph offset: `u64`
  - Graph length: `u64`
  - Data offset: `u64`
  - Data length: `u64`
  - Id map offset: `u64`
  - Id map length: `u64`
  - SHA-256 checksum: 32 bytes
- Payload blocks
  - Model id (UTF-8 bytes)
  - HNSW graph dump bytes
  - HNSW data dump bytes
  - Id map JSON bytes

The checksum is computed over the model id, graph, data, and id map bytes.

## Migration behavior

- On load, the system prefers the container file.
- If only legacy files exist, it loads them and rewrites the container next to
  the database, updating `ann_index_meta`.
- Legacy files are not deleted automatically.

## Cleanup

If disk space is a concern, remove the legacy `ann/` directory after verifying
the container file loads correctly. There is no automated cleanup step yet.
