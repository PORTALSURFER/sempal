use crate::native_app::sample_library::committed_file_mutations::FileMutationChange;
use crate::native_app::sample_library::committed_file_mutations::RevisionFirstCursor;
use crate::native_app::transaction_history::NativeTransactionHistory;

#[derive(Default)]
pub(in crate::native_app) struct TransactionState {
    pub(in crate::native_app) history: NativeTransactionHistory,
    pub(in crate::native_app) restoring: bool,
    pub(in crate::native_app) latest_committed_mutation:
        std::collections::HashMap<String, RevisionFirstCursor>,
    pub(in crate::native_app) pending_file_mutations: Vec<FileMutationChange>,
    pub(in crate::native_app) pending_file_mutation_failures: Vec<String>,
    pub(in crate::native_app) pending_file_mutation_attempted: bool,
}
