use super::*;

#[test]
fn fallback_get_rejects_corrupted_payload() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    set_env_key(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    std::fs::write(store.fallback_token_path(), b"short").unwrap();
    let err = store.fallback_get().unwrap_err();
    match err {
        IssueTokenStoreError::Decode(_) => {}
        other => panic!("expected decode error, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn fallback_token_file_is_private_on_unix() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    use std::os::unix::fs::PermissionsExt;
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    set_env_key(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
    let token_mode = std::fs::metadata(store.fallback_token_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(token_mode, 0o600);
}

#[test]
fn fallback_get_rejects_oversized_payload() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    set_env_key(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    let oversized = vec![0u8; (MAX_FALLBACK_TOKEN_BYTES + 1) as usize];
    std::fs::write(store.fallback_token_path(), oversized).unwrap();
    let err = store.fallback_get().unwrap_err();
    match err {
        IssueTokenStoreError::Decode(message) => {
            assert!(message.contains("exceeds"));
        }
        other => panic!("expected decode error, got {other:?}"),
    }
}

#[test]
fn fallback_get_clears_unreadable_payload() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    runtime.set_var("WAVECRATE_DISABLE_KEYRING", "1");
    allow_fallback(&mut runtime);
    set_env_key(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    let mut payload = vec![0u8; 12];
    payload.extend_from_slice(&[1u8; 16]);
    std::fs::write(store.fallback_token_path(), payload).unwrap();
    assert_eq!(store.fallback_get().unwrap(), None);
    assert!(!store.fallback_token_path().exists());
}
