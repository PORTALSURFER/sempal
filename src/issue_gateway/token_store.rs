//! Token storage for issue access, preferring the OS keyring with an opt-in
//! obfuscated file fallback for platforms where the keyring is unavailable.
//! The fallback stores the encryption key alongside the ciphertext, so it
//! provides only lightweight protection at rest and must be explicitly enabled.

use crate::app_dirs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const KEYRING_SERVICE: &str = "sempal";
const KEYRING_KEY: &str = "sempal_github_issue_token";
const FALLBACK_ALLOW_ENV: &str = "SEMPAL_ALLOW_FALLBACK_TOKEN_STORAGE";

static FALLBACK_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);

/// Errors returned by the issue token storage backend.
#[derive(Debug, thiserror::Error)]
pub enum IssueTokenStoreError {
    #[error("Token store unavailable: {0}")]
    Unavailable(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Crypto error: {0}")]
    Crypto(String),
    #[error("Decode error: {0}")]
    Decode(String),
    #[error("App dir error: {0}")]
    AppDir(#[from] crate::app_dirs::AppDirError),
}

/// Stores the issue token in the OS keyring with an opt-in encrypted file fallback.
///
/// The fallback stores the encryption key next to the ciphertext, so it only
/// offers obfuscation if an attacker can read the filesystem.
#[derive(Clone, Debug)]
pub struct IssueTokenStore {
    fallback_dir: PathBuf,
}

impl IssueTokenStore {
    /// Create a token store rooted in the configured app directory.
    pub fn new() -> Result<Self, IssueTokenStoreError> {
        let fallback_dir = app_dirs::app_root_dir()?.join("secrets");
        std::fs::create_dir_all(&fallback_dir)?;
        Ok(Self { fallback_dir })
    }

    /// Load the token from the keyring or the opt-in fallback storage if allowed.
    pub fn get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        match self.try_keyring_get() {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) => {
                if fallback_allowed() {
                    self.fallback_get()
                } else {
                    Ok(None)
                }
            }
            Err(keyring_err) => {
                if fallback_allowed() {
                    // Keyring failed, try fallback if explicitly enabled.
                    self.fallback_get()
                } else {
                    Err(IssueTokenStoreError::Unavailable(format!(
                        "Keyring unavailable ({keyring_err}). Fallback storage is disabled; set {FALLBACK_ALLOW_ENV}=1 to allow obfuscated file storage."
                    )))
                }
            }
        }
    }

    /// Store the token, preferring the OS keyring and using the fallback only
    /// when explicitly enabled.
    pub fn set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        let token = token.trim();
        if token.is_empty() {
            return self.delete();
        }

        let keyring_err = match self.try_keyring_set(token) {
            Ok(_) => {
                // Verify it can be read back - with retries for flaky backends
                let mut last_error = None;
                for _ in 0..5 {
                    match self.try_keyring_get() {
                        Ok(Some(stored)) if stored == token => {
                            let _ = self.fallback_delete();
                            return Ok(());
                        }
                        Ok(Some(stored)) => {
                            last_error = Some(IssueTokenStoreError::Unavailable(
                                format!("Keyring set succeeded but read back mismatch (got {} bytes, expected {}).", 
                                    stored.len(), token.len())
                            ));
                        }
                        Ok(None) => {
                            last_error = Some(IssueTokenStoreError::Unavailable(
                                "Keyring set reported success but item was not found immediately after.".into(),
                            ));
                        }
                        Err(e) => {
                            last_error = Some(e);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                // If we get here, keyring failed after retries. Use fallback automatically.
                last_error
            }
            Err(e) => Some(e),
        };

        if fallback_allowed() {
            // Keyring failed, use fallback storage only when explicitly enabled.
            match self.fallback_set(token) {
                Ok(_) => Ok(()),
                Err(fallback_err) => Err(fallback_err),
            }
        } else {
            let keyring_error = keyring_err
                .as_ref()
                .map(|err| err.to_string())
                .unwrap_or_else(|| "unknown keyring error".into());
            Err(IssueTokenStoreError::Unavailable(format!(
                "Keyring unavailable ({keyring_error}). Fallback storage is disabled; set {FALLBACK_ALLOW_ENV}=1 to allow obfuscated file storage."
            )))
        }
    }

    /// Store the token and verify it can be read back.
    pub fn set_and_verify(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        self.set(token)
    }

    /// Remove the token from all storage backends.
    pub fn delete(&self) -> Result<(), IssueTokenStoreError> {
        let _ = self.try_keyring_delete();
        let _ = self.fallback_delete();
        Ok(())
    }

    fn try_keyring_get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        if keyring_disabled() {
            return Ok(None);
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY)
            .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(IssueTokenStoreError::Unavailable(err.to_string())),
        }
    }

    fn try_keyring_set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        if keyring_disabled() {
            return Err(IssueTokenStoreError::Unavailable("keyring disabled".into()));
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY)
            .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))?;
        entry
            .set_password(token)
            .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))
    }

    fn try_keyring_delete(&self) -> Result<(), IssueTokenStoreError> {
        if keyring_disabled() {
            return Ok(());
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_KEY)
            .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))?;
        let _ = entry.delete_credential();
        Ok(())
    }

    fn fallback_token_path(&self) -> PathBuf {
        self.fallback_dir.join("github_issue_token.bin")
    }

    fn fallback_key_path(&self) -> PathBuf {
        self.fallback_dir.join("encryption.key")
    }

    /// Ensures the fallback encryption key exists, generating it if necessary.
    /// Returns the 32-byte encryption key.
    fn ensure_fallback_key(&self) -> Result<[u8; 32], IssueTokenStoreError> {
        let key_path = self.fallback_key_path();
        
        // Try to read existing key
        if key_path.exists() {
            let key_bytes = std::fs::read(&key_path)?;
            if key_bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&key_bytes);
                return Ok(key);
            }
            // Invalid key file, regenerate
        }

        // Generate new random key
        let key_bytes = random_bytes(32)?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);

        // Store it securely
        write_private_file(&key_path, &key_bytes)?;

        Ok(key)
    }



    fn fallback_get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        if !fallback_allowed() {
            return Err(IssueTokenStoreError::Unavailable(format!(
                "Fallback storage disabled; set {FALLBACK_ALLOW_ENV}=1 to allow obfuscated file storage."
            )));
        }
        let token_path = self.fallback_token_path();
        if !token_path.exists() {
            return Ok(None);
        }
        warn_fallback_active();
        let key = self.ensure_fallback_key()?;
        let data = std::fs::read(token_path)?;
        if data.len() < 12 {
            return Err(IssueTokenStoreError::Decode("token file too short".into()));
        }
        let (nonce, ciphertext) = data.split_at(12);
        let plaintext = decrypt(&key, nonce, ciphertext)?;
        let token = String::from_utf8(plaintext)
            .map_err(|err| IssueTokenStoreError::Decode(err.to_string()))?;
        Ok(Some(token))
    }

    fn fallback_set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        if !fallback_allowed() {
            return Err(IssueTokenStoreError::Unavailable(format!(
                "Fallback storage disabled; set {FALLBACK_ALLOW_ENV}=1 to allow obfuscated file storage."
            )));
        }
        warn_fallback_active();
        let key = self.ensure_fallback_key()?;
        let nonce = random_bytes(12)?;
        let ciphertext = encrypt(&key, &nonce, token.as_bytes())?;
        let mut payload = Vec::with_capacity(nonce.len() + ciphertext.len());
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);
        write_private_file(&self.fallback_token_path(), &payload)?;
        Ok(())
    }

    fn fallback_delete(&self) -> Result<(), IssueTokenStoreError> {
        let _ = std::fs::remove_file(self.fallback_token_path());
        Ok(())
    }
}

fn keyring_disabled() -> bool {
    env_var_truthy("SEMPAL_DISABLE_KEYRING")
}

fn fallback_allowed() -> bool {
    env_var_truthy(FALLBACK_ALLOW_ENV)
}

fn env_var_truthy(key: &str) -> bool {
    std::env::var(key)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn warn_fallback_active() {
    if FALLBACK_WARNING_EMITTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        tracing::warn!(
            "Fallback token storage enabled; the encryption key is stored next to the ciphertext and only provides obfuscation."
        );
    }
}

fn random_bytes(len: usize) -> Result<Vec<u8>, IssueTokenStoreError> {
    let mut out = vec![0u8; len];
    use rand::TryRngCore;
    rand::rngs::OsRng
        .try_fill_bytes(&mut out)
        .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))?;
    Ok(out)
}

/// Write a file with restricted permissions using an atomic swap on supported platforms.
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), IssueTokenStoreError> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| {
        IssueTokenStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "token path has no parent directory",
        ))
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        IssueTokenStoreError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "token path has no file name",
        ))
    })?;

    let mut last_err = None;
    for _ in 0..5 {
        let suffix = random_hex(6)?;
        let tmp_path = dir.join(format!("{}.tmp-{}", file_name.to_string_lossy(), suffix));
        let mut open_options = std::fs::OpenOptions::new();
        open_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open_options.mode(0o600);
        }
        match open_options.open(&tmp_path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                drop(file);
                replace_file(&tmp_path, path)?;
                #[cfg(target_os = "windows")]
                {
                    harden_windows_permissions(path);
                }
                sync_parent_dir(dir)?;
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                last_err = Some(err);
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }

    Err(IssueTokenStoreError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!(
            "failed to create temporary file for {}: {}",
            path.display(),
            last_err
                .as_ref()
                .map(|err| err.to_string())
                .unwrap_or_else(|| "unknown error".into())
        ),
    )))
}

fn replace_file(temp_path: &Path, path: &Path) -> Result<(), IssueTokenStoreError> {
    match std::fs::rename(temp_path, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            #[cfg(target_os = "windows")]
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                std::fs::remove_file(path)?;
                std::fs::rename(temp_path, path)?;
                return Ok(());
            }
            Err(err.into())
        }
    }
}

fn sync_parent_dir(dir: &Path) -> Result<(), IssueTokenStoreError> {
    #[cfg(unix)]
    {
        let dir_handle = std::fs::File::open(dir)?;
        dir_handle.sync_all()?;
    }
    Ok(())
}

fn random_hex(len: usize) -> Result<String, IssueTokenStoreError> {
    let bytes = random_bytes(len)?;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut out, "{:02x}", byte).expect("writing to String should not fail");
    }
    Ok(out)
}

#[cfg(target_os = "windows")]
/// Apply best-effort hiding/readonly attributes for the fallback token file.
/// This is not equivalent to ACLs but avoids a visible plaintext file.
fn harden_windows_permissions(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
        },
        core::PCWSTR,
    };
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let _ = unsafe {
        SetFileAttributesW(
            PCWSTR(wide.as_ptr()),
            FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_READONLY,
        )
    };
}

fn encrypt(key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, IssueTokenStoreError> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    let cipher = chacha20poly1305::ChaCha20Poly1305::new_from_slice(key)
        .map_err(|err| IssueTokenStoreError::Crypto(err.to_string()))?;
    let nonce = chacha20poly1305::Nonce::from_slice(nonce);
    cipher
        .encrypt(nonce, plaintext)
        .map_err(|err| IssueTokenStoreError::Crypto(err.to_string()))
}

fn decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, IssueTokenStoreError> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    let cipher = chacha20poly1305::ChaCha20Poly1305::new_from_slice(key)
        .map_err(|err| IssueTokenStoreError::Crypto(err.to_string()))?;
    let nonce = chacha20poly1305::Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|err| IssueTokenStoreError::Crypto(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn allow_fallback() {
        unsafe {
            std::env::set_var(FALLBACK_ALLOW_ENV, "1");
        }
    }

    fn disallow_fallback() {
        unsafe {
            std::env::remove_var(FALLBACK_ALLOW_ENV);
        }
    }

    #[test]
    fn fallback_roundtrip_when_keyring_disabled() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();
        assert_eq!(store.get().unwrap(), None);
        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        assert_eq!(
            store.get().unwrap().as_deref(),
            Some("tok_abcdefghijklmnopqrstuvwxyz")
        );
        store.delete().unwrap();
        assert_eq!(store.get().unwrap(), None);
        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn set_empty_token_clears_storage() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();
        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        store.set("").unwrap();
        assert_eq!(store.get().unwrap(), None);
        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_is_only_used_when_explicitly_allowed() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        let base = tempdir().unwrap();
        let _guard = crate::app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        // Fallback should be disabled unless explicitly allowed.
        let err = store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap_err();
        match err {
            IssueTokenStoreError::Unavailable(message) => {
                assert!(message.contains(FALLBACK_ALLOW_ENV));
            }
            other => panic!("expected unavailable error, got {other:?}"),
        }
        assert!(!store.fallback_token_path().exists());

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
    }

    #[test]
    fn fallback_get_rejects_corrupted_payload() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        std::fs::write(store.fallback_token_path(), b"short").unwrap();
        let err = store.fallback_get().unwrap_err();
        match err {
            IssueTokenStoreError::Decode(_) => {}
            other => panic!("expected decode error, got {other:?}"),
        }

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[cfg(unix)]
    #[test]
    fn fallback_files_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        let token_mode = std::fs::metadata(store.fallback_token_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let key_mode = std::fs::metadata(store.fallback_key_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(token_mode, 0o600);
        assert_eq!(key_mode, 0o600);

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_warns_when_active() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        FALLBACK_WARNING_EMITTED.store(false, Ordering::SeqCst);
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        assert!(FALLBACK_WARNING_EMITTED.load(Ordering::SeqCst));

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }
}
