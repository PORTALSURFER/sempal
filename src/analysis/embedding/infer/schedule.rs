#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BatchSlice {
    pub(super) start: usize,
    pub(super) len: usize,
}

impl BatchSlice {
    pub(super) fn end(self) -> usize {
        self.start + self.len
    }
}

#[allow(dead_code)]
pub(super) fn plan_batch_slices(
    total: usize,
    max_batch: usize,
    batch_enabled: bool,
) -> Vec<BatchSlice> {
    if total == 0 {
        return Vec::new();
    }
    let batch = if batch_enabled { max_batch.max(1) } else { 1 };
    chunk_ranges(total, batch)
}

pub(super) fn chunk_ranges(total: usize, chunk: usize) -> Vec<BatchSlice> {
    if total == 0 {
        return Vec::new();
    }
    let chunk = chunk.max(1);
    let mut slices = Vec::new();
    let mut start = 0;
    while start < total {
        let len = std::cmp::min(chunk, total - start);
        slices.push(BatchSlice { start, len });
        start += len;
    }
    slices
}
