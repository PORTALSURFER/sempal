use crate::app_dirs;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "sempal";
const KEYRING_KEY: &str = "sempal_github_issue_token";
const FALLBACK_ENABLE_ENV: &str = "SEMPAL_ALLOW_FALLBACK_TOKEN_STORAGE";
const FALLBACK_SECRET_ENV: &str = "SEMPAL_FALLBACK_TOKEN_SECRET";

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

#[derive(Clone, Debug)]
pub struct IssueTokenStore {
    fallback_dir: PathBuf,
    fallback_key: Option<[u8; 32]>,
}

impl IssueTokenStore {
    pub fn new() -> Result<Self, IssueTokenStoreError> {
        let fallback_dir = app_dirs::app_root_dir()?.join("secrets");
        std::fs::create_dir_all(&fallback_dir)?;
        let fallback_key = fallback_key_material(&fallback_dir);
        Ok(Self {
            fallback_dir,
            fallback_key,
        })
    }

    pub fn get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        match self.try_keyring_get() {
            Ok(Some(token)) => Ok(Some(token)),
            Ok(None) => {
                if self.fallback_key.is_some() {
                    self.fallback_get()
                } else {
                    Ok(None)
                }
            }
            Err(keyring_err) => {
                match self.fallback_get() {
                    Ok(Some(token)) => Ok(Some(token)),
                    _ => Err(keyring_err),
                }
            }
        }
    }

    pub fn set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        let token = token.trim();
        if token.is_empty() {
            return self.delete();
        }

        let keyring_err = match self.try_keyring_set(token) {
            Ok(_) => {
                // Verify it can be read back
                match self.try_keyring_get() {
                    Ok(Some(stored)) if stored == token => {
                        let _ = self.fallback_delete();
                        return Ok(());
                    }
                    Ok(Some(stored)) => {
                        Some(IssueTokenStoreError::Unavailable(
                            format!("Keyring set succeeded but read back mismatch (got {} bytes, expected {}).", 
                                stored.len(), token.len())
                        ))
                    }
                    Ok(None) => {
                        Some(IssueTokenStoreError::Unavailable(
                            "Keyring set reported success but item was not found immediately after.".into(),
                        ))
                    }
                    Err(e) => Some(e),
                }
            }
            Err(e) => Some(e),
        };

        match self.fallback_set(token) {
            Ok(_) => Ok(()),
            Err(fallback_err) => {
                if self.fallback_key.is_none() {
                    // Fallback is disabled. Return the keyring error if we have one.
                    if let Some(ke) = keyring_err {
                        return Err(ke);
                    }
                }
                Err(fallback_err)
            }
        }
    }

    /// Store the token and verify it can be read back.
    pub fn set_and_verify(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        self.set(token)
    }

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

    fn fallback_get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        let key = self.fallback_key.ok_or_else(fallback_disabled_error)?;
        let token_path = self.fallback_token_path();
        if !token_path.exists() {
            return Ok(None);
        }
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
        let key = self.fallback_key.ok_or_else(fallback_disabled_error)?;
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
    std::env::var("SEMPAL_DISABLE_KEYRING")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn fallback_opt_in() -> bool {
    std::env::var(FALLBACK_ENABLE_ENV)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn fallback_key_material(fallback_dir: &Path) -> Option<[u8; 32]> {
    if !fallback_opt_in() {
        return None;
    }
    match std::env::var(FALLBACK_SECRET_ENV) {
        Ok(secret) if !secret.is_empty() => Some(derive_fallback_key(&secret, fallback_dir)),
        _ => {
            eprintln!(
                "Warning: fallback token storage enabled ({}), but {} is not set; fallback disabled.",
                FALLBACK_ENABLE_ENV, FALLBACK_SECRET_ENV
            );
            None
        }
    }
}

fn derive_fallback_key(secret: &str, fallback_dir: &Path) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(fallback_dir.as_os_str().to_string_lossy().as_bytes());
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

fn fallback_disabled_error() -> IssueTokenStoreError {
    IssueTokenStoreError::Unavailable(
        "Fallback token storage disabled; set \
         SEMPAL_ALLOW_FALLBACK_TOKEN_STORAGE=1 and \
         SEMPAL_FALLBACK_TOKEN_SECRET to enable an encrypted fallback."
            .to_string(),
    )
}

fn random_bytes(len: usize) -> Result<Vec<u8>, IssueTokenStoreError> {
    let mut out = vec![0u8; len];
    use rand::TryRngCore;
    rand::rngs::OsRng
        .try_fill_bytes(&mut out)
        .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))?;
    Ok(out)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), IssueTokenStoreError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(target_os = "windows")]
    {
        harden_windows_permissions(path);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
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

    #[test]
    fn fallback_roundtrip_when_keyring_disabled() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
            std::env::set_var(FALLBACK_ENABLE_ENV, "1");
            std::env::set_var(FALLBACK_SECRET_ENV, "super_secret_token_key");
        }
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
            std::env::remove_var(FALLBACK_ENABLE_ENV);
            std::env::remove_var(FALLBACK_SECRET_ENV);
        }
    }

    #[test]
    fn set_empty_token_clears_storage() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
            std::env::set_var(FALLBACK_ENABLE_ENV, "1");
            std::env::set_var(FALLBACK_SECRET_ENV, "super_secret_token_key");
        }
        let base = tempdir().unwrap();
        let _guard = app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();
        store.set("tok_abcdefghijklmnopqrstuvwxyz").unwrap();
        store.set("").unwrap();
        assert_eq!(store.get().unwrap(), None);
        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
            std::env::remove_var(FALLBACK_ENABLE_ENV);
            std::env::remove_var(FALLBACK_SECRET_ENV);
        }
    }

    #[test]
    fn get_none_when_both_empty_and_fallback_disabled() {
        unsafe {
            std::env::set_var("SEMPAL_DISABLE_KEYRING", "1");
            std::env::remove_var(FALLBACK_ENABLE_ENV);
        }
        let base = tempdir().unwrap();
        let _guard = crate::app_dirs::ConfigBaseGuard::set(base.path().to_path_buf());
        let store = IssueTokenStore::new().unwrap();
        
        // This should return Ok(None), not Err
        assert_eq!(store.get().unwrap(), None);
        
        unsafe {
            std::env::remove_var("SEMPAL_DISABLE_KEYRING");
        }
    }
}
