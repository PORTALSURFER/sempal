use super::*;

#[test]
fn fallback_requires_env_key_without_keyring() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    let err = store.set("tok_file_fallback").unwrap_err();
    match err {
        IssueTokenStoreError::Unavailable(message) => {
            assert!(message.contains(FALLBACK_KEY_ENV_VAR));
        }
        other => panic!("expected unavailable error, got {other:?}"),
    }
}

#[test]
fn malformed_env_fallback_key_is_rejected() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    runtime.set_var(FALLBACK_KEY_ENV_VAR, "not-hex");
    allow_fallback(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    let err = store.set("tok_file_fallback").unwrap_err();
    match err {
        IssueTokenStoreError::Decode(message) => {
            assert!(message.contains(FALLBACK_KEY_ENV_VAR));
        }
        other => panic!("expected decode error, got {other:?}"),
    }
}

#[test]
fn fallback_works_with_env_key() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    set_env_key(&mut runtime);

    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    store.set("tok_env_fallback").unwrap();
    assert_eq!(store.get().unwrap().as_deref(), Some("tok_env_fallback"));

    assert!(!store.legacy_fallback_key_path().exists());

    store.delete().unwrap();
}
