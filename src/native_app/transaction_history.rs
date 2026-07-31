mod app_state;
mod capacity_gate;
mod context;
mod file_io;
#[cfg(test)]
mod generic;
mod native;
pub(in crate::native_app) mod operation_journal;
pub(in crate::native_app) mod publication;
mod summary;

pub(in crate::native_app) use capacity_gate::RejectedBeforeIntent;
pub(in crate::native_app) use capacity_gate::open_no_follow_path;
pub(in crate::native_app) use context::TransactionContext;
#[cfg(test)]
pub(in crate::native_app) use file_io::HistoryFileIoOutput;
pub(in crate::native_app) use file_io::{
    HistoryFileAction, HistoryFileIoCommand, HistoryFileIoDirection, HistoryFileIoResult,
    HistoryFileIoRoute,
};
pub(in crate::native_app) use native::NativeTransactionHistory;
pub(in crate::native_app) use summary::{
    TransactionApplied, TransactionListItem, TransactionListState,
};

use crate::native_app::ui::ids as widget_ids;

pub(in crate::native_app) const TRANSACTION_LIST_MODAL_ID: u64 =
    widget_ids::TRANSACTION_LIST_MODAL_ID;

const DEFAULT_TRANSACTION_LIMIT: usize = 128;

pub(in crate::native_app) type TransactionResult = Result<(), String>;
