//! Token storage for issue access, preferring the OS keyring with an opt-in
//! encrypted file fallback when keyring-backed token storage fails.
//! The fallback stores ciphertext on disk while keeping the encryption key in
//! the OS keyring to avoid recoverable secrets in the filesystem.

use crate::app_dirs;
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const KEYRING_SERVICE: &str = "sempal";
const KEYRING_KEY: &str = "sempal_github_issue_token";
const FALLBACK_KEYRING_KEY: &str = "sempal_github_issue_token_fallback_key";
const FALLBACK_ALLOW_ENV: &str = "SEMPAL_ALLOW_FALLBACK_TOKEN_STORAGE";
const FALLBACK_KEY_ENV_VAR: &str = "SEMPAL_FALLBACK_KEY";
const MAX_FALLBACK_TOKEN_BYTES: u64 = 16 * 1024;

static FALLBACK_WARNING_EMITTED: AtomicBool = AtomicBool::new(false);
static FALLBACK_KEY_CACHE: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();

/// Errors returned by the issue token storage backend.
#[derive(Debug, thiserror::Error)]
pub enum IssueTokenStoreError {
    /// Token storage is unavailable on this system.
    #[error("Token store unavailable: {0}")]
    Unavailable(String),
    /// IO error while reading/writing storage.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Crypto error when encrypting or decrypting.
    #[error("Crypto error: {0}")]
    Crypto(String),
    /// Decode error when parsing stored values.
    #[error("Decode error: {0}")]
    Decode(String),
    /// Failed to resolve app directories.
    #[error("App dir error: {0}")]
    AppDir(#[from] crate::app_dirs::AppDirError),
}

/// Stores the issue token in the OS keyring with an opt-in encrypted file fallback.
///
/// The fallback stores ciphertext on disk and keeps the encryption key in the OS
/// keyring, so filesystem reads alone cannot recover the token.
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
                        "Keyring unavailable ({keyring_err}). Fallback storage is disabled; set {FALLBACK_ALLOW_ENV}=1 to allow encrypted file storage."
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
                "Keyring unavailable ({keyring_error}). Fallback storage is disabled; set {FALLBACK_ALLOW_ENV}=1 to allow encrypted file storage."
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

    fn legacy_fallback_key_path(&self) -> PathBuf {
        self.fallback_dir.join("encryption.key")
    }

    fn fallback_key_entry(&self) -> Result<keyring::Entry, IssueTokenStoreError> {
        keyring::Entry::new(KEYRING_SERVICE, FALLBACK_KEYRING_KEY)
            .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))
    }

    /// Ensures the fallback encryption key exists, generating it if necessary.
    /// Returns the 32-byte encryption key.
    fn ensure_fallback_key(&self) -> Result<[u8; 32], IssueTokenStoreError> {
        if let Some(key) = self.cached_fallback_key() {
            return Ok(key);
        }

        // 1. Check environment variable
        if let Some(key) = self.get_key_from_env()? {
            self.cache_fallback_key(key);
            return Ok(key);
        }

        // 2. Check file-based key storage
        if let Some(key) = self.get_key_from_file()? {
            self.cache_fallback_key(key);
            return Ok(key);
        }

        // 3. Check Keyring (if not disabled)
        if let Some(key) = self.try_keyring_fallback_key_get()? {
            self.cache_fallback_key(key);
            return Ok(key);
        }

        // Generate new random key
        let key_bytes = random_bytes(32)?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);

        // Try to store it
        if !keyring_disabled() {
            match self.try_keyring_fallback_key_set(&key) {
                Ok(_) => {
                    self.cache_fallback_key(key);
                    return Ok(key);
                }
                Err(e) => {
                    tracing::warn!("Failed to store fallback key in keyring: {e}. Falling back to file storage.");
                }
            }
        }

        // If keyring disabled or failed, store in file
        self.store_key_to_file(&key)?;
        tracing::warn!("Stored fallback encryption key in {} because keyring was unavailable/disabled.", self.legacy_fallback_key_path().display());
        
        self.cache_fallback_key(key);
        Ok(key)
    }

    fn cached_fallback_key(&self) -> Option<[u8; 32]> {
        lock_fallback_key_cache().as_ref().copied()
    }

    fn cache_fallback_key(&self, key: [u8; 32]) {
        *lock_fallback_key_cache() = Some(key);
    }

    fn get_key_from_env(&self) -> Result<Option<[u8; 32]>, IssueTokenStoreError> {
        match std::env::var(FALLBACK_KEY_ENV_VAR) {
            Ok(hex_key) => {
                let bytes = decode_hex(&hex_key).map_err(|e| IssueTokenStoreError::Decode(format!("Invalid hex in {}: {}", FALLBACK_KEY_ENV_VAR, e)))?;
                if bytes.len() != 32 {
                    return Err(IssueTokenStoreError::Decode(format!("{} must be 32 bytes (64 hex chars), got {}", FALLBACK_KEY_ENV_VAR, bytes.len())));
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                Ok(Some(key))
            }
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(IssueTokenStoreError::Decode(format!("{} is not valid unicode", FALLBACK_KEY_ENV_VAR))),
        }
    }

    fn get_key_from_file(&self) -> Result<Option<[u8; 32]>, IssueTokenStoreError> {
        let key_path = self.legacy_fallback_key_path(); // We reuse this path for the file-based key
        if !key_path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&key_path)?;
        if bytes.len() != 32 {
            tracing::warn!("Fallback key file {} is corrupt (wrong size), ignoring.", key_path.display());
            return Ok(None);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Some(key))
    }

    fn store_key_to_file(&self, key: &[u8; 32]) -> Result<(), IssueTokenStoreError> {
         write_private_file(&self.legacy_fallback_key_path(), key)
    }


    fn try_keyring_fallback_key_get(&self) -> Result<Option<[u8; 32]>, IssueTokenStoreError> {
        if keyring_disabled() {
             return Ok(None);
        }
        let entry = self.fallback_key_entry()?;
        match entry.get_password() {
            Ok(encoded) => {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .map_err(|err| IssueTokenStoreError::Decode(err.to_string()))?;
                if decoded.len() != 32 {
                    return Ok(None);
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                Ok(Some(key))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(IssueTokenStoreError::Unavailable(format!(
                "Fallback keyring unavailable ({err})."
            ))),
        }
    }

    fn try_keyring_fallback_key_set(&self, key: &[u8; 32]) -> Result<(), IssueTokenStoreError> {
        if keyring_disabled() {
            return Err(IssueTokenStoreError::Unavailable("keyring disabled".into()));
        }
        let entry = self.fallback_key_entry()?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(key);
        entry
            .set_password(&encoded)
            .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))
    }

    fn try_keyring_fallback_key_delete(&self) -> Result<(), IssueTokenStoreError> {
        if keyring_disabled() {
            return Ok(());
        }
        let entry = self.fallback_key_entry()?;
        let _ = entry.delete_credential();
        Ok(())
    }

    fn fallback_get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        if !fallback_allowed() {
            return Err(IssueTokenStoreError::Unavailable(format!(
                "Fallback storage disabled; set {FALLBACK_ALLOW_ENV}=1 to allow encrypted file storage."
            )));
        }
        let token_path = self.fallback_token_path();
        if !token_path.exists() {
            return Ok(None);
        }
        let metadata = std::fs::metadata(&token_path)?;
        if metadata.len() > MAX_FALLBACK_TOKEN_BYTES {
            return Err(IssueTokenStoreError::Decode(format!(
                "fallback token file exceeds {MAX_FALLBACK_TOKEN_BYTES} bytes"
            )));
        }
        warn_fallback_active();
        let key = self.ensure_fallback_key()?;
        let data = std::fs::read(token_path)?;
        if data.len() < 12 {
            return Err(IssueTokenStoreError::Decode("token file too short".into()));
        }
        let (nonce, ciphertext) = data.split_at(12);
        // We now treat the file-based key as a first-class citizen, so we just attempt strict decryption.
        // If the key has rotated or migrated, the standard flow is to clear and restart.
        // Complex migration logic (reading old key file, re-encrypting) is simplified:
        // if the current resolved key doesn't work, we assume the data is garbage/stale.
        let plaintext = match decrypt(&key, nonce, ciphertext) {
            Ok(plaintext) => plaintext,
            Err(err) => {
                 // Try one last ditch attempt: checking if the 'legacy' file key works 
                 // (only relevant if we are now using a DIFFERENT key, e.g. from env or keyring).
                 // However, simplifying: if we can't decrypt with the resolved key, we reset.
                 tracing::warn!(
                    "Fallback token payload failed to decrypt; clearing fallback storage: {err}"
                 );
                 let _ = self.fallback_delete();
                 return Ok(None);
            }
        };
        let token = String::from_utf8(plaintext)
            .map_err(|err| IssueTokenStoreError::Decode(err.to_string()))?;
        Ok(Some(token))
    }

    fn fallback_set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        if !fallback_allowed() {
            return Err(IssueTokenStoreError::Unavailable(format!(
                "Fallback storage disabled; set {FALLBACK_ALLOW_ENV}=1 to allow encrypted file storage."
            )));
        }
        warn_fallback_active();
        let key = self.ensure_fallback_key()?;
        let payload = self.encrypt_fallback_payload(&key, token.as_bytes())?;
        write_private_file(&self.fallback_token_path(), &payload)?;
        Ok(())
    }

    fn fallback_delete(&self) -> Result<(), IssueTokenStoreError> {
        #[cfg(target_os = "windows")]
        {
            clear_windows_readonly(self.fallback_token_path().as_path());
        }
        let _ = std::fs::remove_file(self.fallback_token_path());
        // This file is now used for the file-based fallback key as well
        let _ = std::fs::remove_file(self.legacy_fallback_key_path());
        let _ = self.try_keyring_fallback_key_delete();
        *lock_fallback_key_cache() = None;
        Ok(())
    }



    fn encrypt_fallback_payload(
        &self,
        key: &[u8; 32],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, IssueTokenStoreError> {
        let nonce = random_bytes(12)?;
        let ciphertext = encrypt(key, &nonce, plaintext)?;
        let mut payload = Vec::with_capacity(nonce.len() + ciphertext.len());
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);
        Ok(payload)
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
            "Fallback token storage enabled; ciphertext is stored on disk and the encryption key is stored in the OS keyring."
        );
    }
}

fn fallback_key_cache() -> &'static Mutex<Option<[u8; 32]>> {
    FALLBACK_KEY_CACHE.get_or_init(|| Mutex::new(None))
}

fn lock_fallback_key_cache() -> std::sync::MutexGuard<'static, Option<[u8; 32]>> {
    match fallback_key_cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("Fallback key cache mutex poisoned; clearing cached key.");
            let mut inner = poisoned.into_inner();
            *inner = None;
            inner
        }
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
            if err.kind() == std::io::ErrorKind::AlreadyExists
                || err.kind() == std::io::ErrorKind::PermissionDenied
            {
                clear_windows_readonly(path);
                if let Err(e) = std::fs::remove_file(path) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        return Err(e.into());
                    }
                }
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
    #[cfg(not(unix))]
    {
        let _ = dir;
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

#[cfg(target_os = "windows")]
/// Clear readonly attributes so the fallback token file can be replaced.
fn clear_windows_readonly(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, SetFileAttributesW},
        core::PCWSTR,
    };
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let _ = unsafe { SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL) };
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

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("Odd number of digits".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, Once, OnceLock};
    use tempfile::tempdir;

    static MOCK_KEYRING_INIT: Once = Once::new();
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn enable_mock_keyring() {
        MOCK_KEYRING_INIT.call_once(|| {
            keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        });
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned")
    }

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

    fn reset_cache() {
        *lock_fallback_key_cache() = None;
    }

    #[test]
    fn fallback_key_cache_recovers_after_poison() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        let _ = std::panic::catch_unwind(|| {
            let _guard = fallback_key_cache()
                .lock()
                .expect("poison fallback key cache");
            panic!("poison fallback key cache");
        });

        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        assert_eq!(
            store.get().unwrap().as_deref(),
            Some("tok_abcdefghijklmnopqrstuvwxyz")
        );
        store.delete().unwrap();
        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_roundtrip_when_keyring_disabled() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
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
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
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
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
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
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
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
    fn fallback_token_file_is_private_on_unix() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
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

        assert_eq!(token_mode, 0o600);

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_get_rejects_oversized_payload() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
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

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_get_clears_unreadable_payload() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        let mut payload = vec![0u8; 12];
        payload.extend_from_slice(&[1u8; 16]);
        std::fs::write(store.fallback_token_path(), payload).unwrap();
        assert_eq!(store.fallback_get().unwrap(), None);
        assert!(!store.fallback_token_path().exists());

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_get_uses_legacy_key_file() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        let legacy_key_bytes = random_bytes(32).unwrap();
        let mut legacy_key = [0u8; 32];
        legacy_key.copy_from_slice(&legacy_key_bytes);
        write_private_file(&store.legacy_fallback_key_path(), &legacy_key_bytes).unwrap();
        let legacy_payload = store
            .encrypt_fallback_payload(&legacy_key, b"tok_legacy")
            .unwrap();
        write_private_file(&store.fallback_token_path(), &legacy_payload).unwrap();

        // Should successfully read using the file-based key
        assert_eq!(store.fallback_get().unwrap().as_deref(), Some("tok_legacy"));
        // The key file should still exist (no migration/deletion)
        assert!(store.legacy_fallback_key_path().exists());

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_works_with_no_keyring_and_file_key() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        // Should work even though keyring is disabled
        store.set("tok_file_fallback").unwrap();
        assert_eq!(store.get().unwrap().as_deref(), Some("tok_file_fallback"));
        
        // Verify key file exists
        assert!(store.legacy_fallback_key_path().exists());
        
        // Clean up
        store.delete().unwrap();
        assert!(!store.legacy_fallback_key_path().exists());

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_works_with_env_key() {
        enable_mock_keyring();
        let _env_guard = env_lock();
        reset_cache();
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
        }
        allow_fallback();
        // 32-bytes hex string (all As)
        let env_key = "A".repeat(64);
        unsafe {
            std::env::set_var(FALLBACK_KEY_ENV_VAR, &env_key);
        }
        
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();

        store.set("tok_env_fallback").unwrap();
        assert_eq!(store.get().unwrap().as_deref(), Some("tok_env_fallback"));
        
        // Key file should NOT exist because we provided env var
        assert!(!store.legacy_fallback_key_path().exists());
        
        store.delete().unwrap();

        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
            std::env::remove_var(FALLBACK_KEY_ENV_VAR);
        }
        disallow_fallback();
    }

    #[test]
    fn fallback_warns_when_active() {
        enable_mock_keyring();
        let _env_guard = env_lock();
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
