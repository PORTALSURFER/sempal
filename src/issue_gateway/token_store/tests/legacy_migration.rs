use super::*;

#[test]
fn fallback_get_migrates_legacy_key_file() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    allow_fallback(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();

    let legacy_key_bytes = crypto::random_bytes(32).unwrap();
    let mut legacy_key = [0u8; 32];
    legacy_key.copy_from_slice(&legacy_key_bytes);
    file_io::write_private_file(&store.legacy_fallback_key_path(), &legacy_key_bytes).unwrap();
    let legacy_payload = store
        .encrypt_fallback_payload(&legacy_key, b"tok_legacy")
        .unwrap();
    file_io::write_private_file(&store.fallback_token_path(), &legacy_payload).unwrap();

    assert_eq!(store.fallback_get().unwrap().as_deref(), Some("tok_legacy"));
    assert!(!store.legacy_fallback_key_path().exists());
}

#[test]
fn corrupt_legacy_fallback_key_file_is_removed() {
    enable_mock_keyring();
    let mut runtime = env_lock();
    reset_cache();
    allow_fallback(&mut runtime);
    clear_env_key(&mut runtime);
    let base = tempdir().unwrap();
    let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
    let store = IssueTokenStore::new().unwrap();
    std::fs::write(store.legacy_fallback_key_path(), b"bad").unwrap();

    assert_eq!(store.get_key_from_file().unwrap(), None);
    assert!(!store.legacy_fallback_key_path().exists());
}
