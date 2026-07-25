use super::*;
use crate::test_runtime::TestRuntimeGuard;
use rusqlite::{OptionalExtension, params};
use tempfile::tempdir;

mod metadata;
mod opening;
mod snapshot;

fn with_home_env_override<T>(home: &Path, test: impl FnOnce() -> T) -> T {
    let mut runtime = TestRuntimeGuard::acquire();
    runtime.set_var("HOME", home);
    runtime.remove_var("HOMEDRIVE");
    runtime.remove_var("HOMEPATH");
    runtime.set_var("USERPROFILE", home);

    test()
}
