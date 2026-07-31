#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum TransactionListState {
    Active,
    Unavailable,
    Undoable,
    Redoable,
}

impl TransactionListState {
    pub(in crate::native_app) fn label(self) -> &'static str {
        match self {
            Self::Active => "Open",
            Self::Unavailable => "Busy",
            Self::Undoable => "Undo",
            Self::Redoable => "Redo",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct TransactionListItem {
    pub(in crate::native_app) id: u64,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) action_count: usize,
    pub(in crate::native_app) action_labels: Vec<String>,
    pub(in crate::native_app) state: TransactionListState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct TransactionApplied {
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) action_count: usize,
}
