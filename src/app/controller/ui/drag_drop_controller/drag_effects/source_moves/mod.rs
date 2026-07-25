//! Cross-source sample move handling for drag/drop.
//!
//! The source-move pipeline is split into:
//! - `plan`: request collection and worker kickoff
//! - `apply_result`: controller-side cache/UI application
//! - `registration`: shared DB registration helpers reused by other drag effects
//! - `worker`: background execution with explicit transaction stages

mod apply_result;
mod plan;
mod registration;
mod worker;
