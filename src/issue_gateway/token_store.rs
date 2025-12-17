use crate::app_dirs;
use std::path::PathBuf;

const KEYRING_SERVICE: &str = "sempal";
const KEYRING_KEY: &str = "sempal_github_issue_token";

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
}

impl IssueTokenStore {
    pub fn new() -> Result<Self, IssueTokenStoreError> {
        let fallback_dir = app_dirs::app_root_dir()?.join("secrets");
        std::fs::create_dir_all(&fallback_dir)?;
        Ok(Self { fallback_dir })
    }

    pub fn get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        if let Some(token) = self.try_keyring_get()? {
            return Ok(Some(token));
        }
        self.fallback_get()
    }

    pub fn set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        let token = token.trim();
        if token.is_empty() {
            return Ok(());
        }
        if self.try_keyring_set(token).is_ok() {
            let _ = self.fallback_delete();
            return Ok(());
        }
        self.fallback_set(token)
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
            Err(_) => Ok(None),
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
        self.fallback_dir.join("github_issue_token.key")
    }

    fn fallback_get(&self) -> Result<Option<String>, IssueTokenStoreError> {
        let token_path = self.fallback_token_path();
        if !token_path.exists() {
            return Ok(None);
        }
        let data = std::fs::read(token_path)?;
        if data.len() < 12 {
            return Err(IssueTokenStoreError::Decode("token file too short".into()));
        }
        let (nonce, ciphertext) = data.split_at(12);
        let key_bytes = std::fs::read(self.fallback_key_path())?;
        if key_bytes.len() != 32 {
            return Err(IssueTokenStoreError::Decode("token key invalid".into()));
        }
        let plaintext = decrypt(&key_bytes, nonce, ciphertext)?;
        let token = String::from_utf8(plaintext).map_err(|err| IssueTokenStoreError::Decode(err.to_string()))?;
        Ok(Some(token))
    }

    fn fallback_set(&self, token: &str) -> Result<(), IssueTokenStoreError> {
        let key_path = self.fallback_key_path();
        let key_bytes = if key_path.exists() {
            std::fs::read(&key_path)?
        } else {
            let bytes = random_bytes(32)?;
            write_private_file(&key_path, &bytes)?;
            bytes
        };
        if key_bytes.len() != 32 {
            return Err(IssueTokenStoreError::Decode("token key invalid".into()));
        }
        let nonce = random_bytes(12)?;
        let ciphertext = encrypt(&key_bytes, &nonce, token.as_bytes())?;
        let mut payload = Vec::with_capacity(nonce.len() + ciphertext.len());
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ciphertext);
        write_private_file(&self.fallback_token_path(), &payload)?;
        Ok(())
    }

    fn fallback_delete(&self) -> Result<(), IssueTokenStoreError> {
        let _ = std::fs::remove_file(self.fallback_token_path());
        let _ = std::fs::remove_file(self.fallback_key_path());
        Ok(())
    }
}

fn keyring_disabled() -> bool {
    std::env::var("SEMPAL_DISABLE_KEYRING")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn random_bytes(len: usize) -> Result<Vec<u8>, IssueTokenStoreError> {
    let mut out = vec![0u8; len];
    use rand::TryRngCore;
    rand::rngs::OsRng
        .try_fill_bytes(&mut out)
        .map_err(|err| IssueTokenStoreError::Unavailable(err.to_string()))?;
    Ok(out)
}

fn write_private_file(path: &PathBuf, bytes: &[u8]) -> Result<(), IssueTokenStoreError> {
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
    Ok(())
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
        }
    }
}
