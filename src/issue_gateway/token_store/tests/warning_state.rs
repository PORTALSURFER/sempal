use super::*;

#[test]
fn fallback_warns_when_active() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    set_env_key(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    fallback_policy::with_fallback_warning_scope_for_tests(|| {
        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        assert!(fallback_policy::fallback_warning_emitted_for_tests());
    });
}

#[test]
fn fallback_warning_scope_restores_nested_state_after_unwind() {
    fallback_policy::with_fallback_warning_scope_for_tests(|| {
        fallback_policy::warn_fallback_active();
        assert!(fallback_policy::fallback_warning_emitted_for_tests());

        let unwind = std::panic::catch_unwind(|| {
            fallback_policy::with_fallback_warning_scope_for_tests(|| {
                assert!(!fallback_policy::fallback_warning_emitted_for_tests());
                fallback_policy::warn_fallback_active();
                panic!("exercise fallback warning scope cleanup");
            });
        });
        assert!(unwind.is_err());
        assert!(fallback_policy::fallback_warning_emitted_for_tests());
    });

    assert!(!fallback_policy::fallback_warning_emitted_for_tests());
}
