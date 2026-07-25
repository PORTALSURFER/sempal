use super::{
    FALLBACK_KEY_ENV_VAR, IssueTokenStore, IssueTokenStoreError, cleanup_result, crypto,
    fallback_policy, fallback_store,
};
use std::sync::{Mutex, OnceLock};

static FALLBACK_KEY_CACHE: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();

impl IssueTokenStore {
    /// Resolve a 32-byte fallback encryption key from cache/env/keyring/legacy file.
    pub(super) fn ensure_fallback_key(&self) -> Result<[u8; 32], IssueTokenStoreError> {
        if let Some(key) = self.cached_fallback_key() {
            return Ok(key);
        }

        if let Some(key) = self.get_key_from_env()? {
            self.remove_legacy_fallback_key_file()?;
            self.cache_fallback_key(key);
            return Ok(key);
        }

        if let Some(key) = self.try_keyring_fallback_key_get()? {
            self.cache_fallback_key(key);
            return Ok(key);
        }

        if !fallback_policy::keyring_disabled()
            && let Some(key) = self.get_key_from_file()?
        {
            self.try_keyring_fallback_key_set(&key)?;
            self.remove_legacy_fallback_key_file()?;
            self.cache_fallback_key(key);
            return Ok(key);
        }

        let key_bytes = crypto::random_bytes(32)?;
        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);

        if fallback_policy::keyring_disabled() {
            return Err(IssueTokenStoreError::Unavailable(format!(
                "Keyring unavailable; set {FALLBACK_KEY_ENV_VAR} to enable fallback token storage."
            )));
        }

        self.try_keyring_fallback_key_set(&key)?;
        self.cache_fallback_key(key);
        Ok(key)
    }

    /// Remove the legacy fallback key file after env/keyring migration paths supersede it.
    pub(super) fn remove_legacy_fallback_key_file(&self) -> Result<(), IssueTokenStoreError> {
        let mut failures = Vec::new();
        fallback_store::remove_optional_file(
            self.legacy_fallback_key_path().as_path(),
            "legacy fallback key",
            &mut failures,
        );
        cleanup_result(failures)
    }

    /// Return the currently cached fallback key, if one has been resolved in-process.
    pub(super) fn cached_fallback_key(&self) -> Option<[u8; 32]> {
        lock_fallback_key_cache().as_ref().copied()
    }

    /// Replace the in-process fallback key cache with the provided key bytes.
    pub(super) fn cache_fallback_key(&self, key: [u8; 32]) {
        *lock_fallback_key_cache() = Some(key);
    }

    /// Parse a hex-encoded key from `WAVECRATE_FALLBACK_KEY` when present.
    pub(super) fn get_key_from_env(&self) -> Result<Option<[u8; 32]>, IssueTokenStoreError> {
        match std::env::var(FALLBACK_KEY_ENV_VAR) {
            Ok(hex_key) => {
                let bytes = crypto::decode_hex(&hex_key).map_err(|e| {
                    IssueTokenStoreError::Decode(format!(
                        "Invalid hex in {}: {}",
                        FALLBACK_KEY_ENV_VAR, e
                    ))
                })?;
                if bytes.len() != 32 {
                    return Err(IssueTokenStoreError::Decode(format!(
                        "{} must be 32 bytes (64 hex chars), got {}",
                        FALLBACK_KEY_ENV_VAR,
                        bytes.len()
                    )));
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                Ok(Some(key))
            }
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(IssueTokenStoreError::Decode(format!(
                "{} is not valid unicode",
                FALLBACK_KEY_ENV_VAR
            ))),
        }
    }

    /// Read the legacy on-disk fallback key file when it exists and is valid.
    pub(super) fn get_key_from_file(&self) -> Result<Option<[u8; 32]>, IssueTokenStoreError> {
        let key_path = self.legacy_fallback_key_path();
        if !key_path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(&key_path)?;
        if bytes.len() != 32 {
            tracing::warn!(
                "Fallback key file {} is corrupt (wrong size), removing.",
                key_path.display()
            );
            self.remove_legacy_fallback_key_file()?;
            return Ok(None);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Some(key))
    }
}

/// Return the process-wide fallback key cache container.
pub(super) fn fallback_key_cache() -> &'static Mutex<Option<[u8; 32]>> {
    FALLBACK_KEY_CACHE.get_or_init(|| Mutex::new(None))
}

/// Lock the process-wide fallback key cache and recover from poisoning.
pub(super) fn lock_fallback_key_cache() -> std::sync::MutexGuard<'static, Option<[u8; 32]>> {
    let cache = fallback_key_cache();
    match cache.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("Fallback key cache mutex poisoned; clearing cached key.");
            let mut inner = poisoned.into_inner();
            *inner = None;
            cache.clear_poison();
            inner
        }
    }
}
