#[derive(Default)]
pub(super) struct ErrorCollector {
    errors: Vec<String>,
    limit: usize,
}

impl ErrorCollector {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            errors: Vec::new(),
            limit,
        }
    }

    pub(super) fn push(&mut self, err: String) {
        if self.errors.len() < self.limit {
            self.errors.push(err);
        }
    }

    pub(super) fn into_vec(self) -> Vec<String> {
        self.errors
    }
}
