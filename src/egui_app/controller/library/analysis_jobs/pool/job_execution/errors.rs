#[derive(Default)]
pub(crate) struct ErrorCollector {
    errors: Vec<String>,
    limit: usize,
}

impl ErrorCollector {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            errors: Vec::new(),
            limit,
        }
    }

    pub(crate) fn push(&mut self, err: String) {
        if self.errors.len() < self.limit {
            self.errors.push(err);
        }
    }

    pub(crate) fn into_vec(self) -> Vec<String> {
        self.errors
    }
}
