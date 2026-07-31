//! Local Vault — encrypted at-rest storage for agent tokens.
//!
//! File format (vault.bin, JSON envelope):
//!   { version: 1, mode: "keyring" | "passphrase", salt_b64?, nonce_b64, ciphertext_b64 }
//! Ciphertext = AES-256-GCM over the JSON payload { tokens: { agent_id: token } }.
//!
//! Key material:
//!   - keyring mode: a random 256-bit data key lives in the OS credential store
//!     (Windows Credential Manager / macOS Keychain / Secret Service).
//!   - passphrase mode: key = Argon2id(passphrase, salt). Nothing usable on disk.
//!
//! Tokens are never written to any other file and never logged.

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use argon2::Argon2;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

const VAULT_VERSION: u32 = 1;
const KEYRING_SERVICE: &str = "com.tragentics.heartbeatcc";
const KEYRING_USER: &str = "vault-key";

/// Argon2id work factors for passphrase mode: 64 MiB (see argon_m_kib), t=3, p=1.
const ARGON_T: u32 = 3;
const ARGON_P: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultState {
    /// No vault file yet — first-run setup required.
    Uninitialized,
    /// Passphrase-mode vault exists but has not been unlocked this session.
    Locked,
    /// Vault open; tokens available to the engine.
    Unlocked,
    /// Keyring mode but the OS entry is missing/unreadable.
    KeyringUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultMode {
    Keyring,
    Passphrase,
}

#[derive(Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    mode: VaultMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    salt_b64: Option<String>,
    nonce_b64: String,
    ciphertext_b64: String,
}

#[derive(Serialize, Deserialize, Default)]
struct VaultPayload {
    tokens: HashMap<String, String>,
}

impl Drop for VaultPayload {
    fn drop(&mut self) {
        for v in self.tokens.values_mut() {
            v.zeroize();
        }
    }
}

pub struct Vault {
    path: PathBuf,
    mode: Option<VaultMode>,
    key: Option<Zeroizing<[u8; 32]>>,
    salt: Option<[u8; 16]>,
    payload: Option<VaultPayload>,
    /// Test hook: when Some, bypasses the OS keyring entirely.
    keyring_override: Option<[u8; 32]>,
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            mode: None,
            key: None,
            salt: None,
            payload: None,
            keyring_override: None,
        }
    }

    #[cfg(test)]
    pub fn with_keyring_override(path: PathBuf, key: [u8; 32]) -> Self {
        let mut v = Self::new(path);
        v.keyring_override = Some(key);
        v
    }

    pub fn state(&self) -> VaultState {
        if self.payload.is_some() {
            return VaultState::Unlocked;
        }
        if !self.path.exists() {
            return VaultState::Uninitialized;
        }
        match self.mode {
            Some(VaultMode::Passphrase) => VaultState::Locked,
            Some(VaultMode::Keyring) => VaultState::KeyringUnavailable,
            None => VaultState::Locked,
        }
    }

    pub fn mode(&self) -> Option<VaultMode> {
        self.mode
    }

    /// Load the envelope (not the secrets) and, in keyring mode, try to
    /// auto-unlock with the OS-stored key. Call once at startup.
    pub fn open(&mut self) -> Result<VaultState, String> {
        if !self.path.exists() {
            return Ok(VaultState::Uninitialized);
        }
        let file = self.read_envelope()?;
        self.mode = Some(file.mode);
        if let Some(salt_b64) = &file.salt_b64 {
            let salt_bytes = B64
                .decode(salt_b64)
                .map_err(|_| "vault file corrupted (salt)".to_string())?;
            let salt: [u8; 16] = salt_bytes
                .try_into()
                .map_err(|_| "vault file corrupted (salt length)".to_string())?;
            self.salt = Some(salt);
        }
        match file.mode {
            VaultMode::Keyring => match self.keyring_key(false) {
                Ok(key) => {
                    self.key = Some(key);
                    // Wrong key or tampered file → fail CLOSED into the trouble
                    // state (UI shows recovery guidance), never a hard error.
                    match self.decrypt_into_memory(&file) {
                        Ok(()) => Ok(VaultState::Unlocked),
                        Err(_) => {
                            self.key = None;
                            Ok(VaultState::KeyringUnavailable)
                        }
                    }
                }
                Err(_) => Ok(VaultState::KeyringUnavailable),
            },
            VaultMode::Passphrase => Ok(VaultState::Locked),
        }
    }

    /// First-run initialization. Creates an empty vault in the chosen mode.
    pub fn initialize(&mut self, mode: VaultMode, passphrase: Option<&str>) -> Result<(), String> {
        if self.path.exists() {
            return Err("Vault already exists".into());
        }
        match mode {
            VaultMode::Keyring => {
                let key = self.keyring_key(true)?;
                self.key = Some(key);
                self.salt = None;
            }
            VaultMode::Passphrase => {
                let pass = passphrase.ok_or("Passphrase required")?;
                validate_passphrase(pass)?;
                let mut salt = [0u8; 16];
                OsRng.fill_bytes(&mut salt);
                self.salt = Some(salt);
                self.key = Some(derive_key(pass, &salt)?);
            }
        }
        self.mode = Some(mode);
        self.payload = Some(VaultPayload::default());
        self.persist()
    }

    /// Unlock a passphrase-mode vault. Wrong passphrase → AEAD failure → error.
    pub fn unlock(&mut self, passphrase: &str) -> Result<(), String> {
        if self.payload.is_some() {
            return Ok(());
        }
        let file = self.read_envelope()?;
        if file.mode != VaultMode::Passphrase {
            return Err("Vault is not passphrase-protected".into());
        }
        let salt = self.salt.ok_or("vault file corrupted (no salt)")?;
        let key = derive_key(passphrase, &salt)?;
        self.key = Some(key);
        match self.decrypt_into_memory(&file) {
            Ok(()) => Ok(()),
            Err(_) => {
                self.key = None;
                Err("Incorrect passphrase".into())
            }
        }
    }

    /// Drop key + secrets from memory (passphrase mode re-lock).
    pub fn lock(&mut self) {
        self.payload = None;
        self.key = None;
    }

    pub fn token_for(&self, agent_id: &str) -> Option<Zeroizing<String>> {
        self.payload
            .as_ref()
            .and_then(|p| p.tokens.get(agent_id))
            .map(|t| Zeroizing::new(t.clone()))
    }

    pub fn insert_token(&mut self, agent_id: &str, token: &str) -> Result<(), String> {
        let payload = self.payload.as_mut().ok_or("Vault is locked")?;
        payload
            .tokens
            .insert(agent_id.to_string(), token.to_string());
        self.persist()
    }

    pub fn remove_token(&mut self, agent_id: &str) -> Result<(), String> {
        let payload = self.payload.as_mut().ok_or("Vault is locked")?;
        if let Some(mut t) = payload.tokens.remove(agent_id) {
            t.zeroize();
        }
        self.persist()
    }

    pub fn token_count(&self) -> usize {
        self.payload.as_ref().map(|p| p.tokens.len()).unwrap_or(0)
    }

    /// Change passphrase (passphrase mode only): re-derive key on a fresh salt
    /// and rewrite the vault.
    pub fn change_passphrase(&mut self, current: &str, next: &str) -> Result<(), String> {
        if self.mode != Some(VaultMode::Passphrase) {
            return Err("Vault is not passphrase-protected".into());
        }
        if self.payload.is_none() {
            self.unlock(current)?;
        } else {
            // Verify current even when already unlocked.
            let salt = self.salt.ok_or("vault file corrupted (no salt)")?;
            let candidate = derive_key(current, &salt)?;
            let held = self.key.as_ref().ok_or("Vault is locked")?;
            if candidate.as_ref() != held.as_ref() {
                return Err("Incorrect passphrase".into());
            }
        }
        validate_passphrase(next)?;
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        self.salt = Some(salt);
        self.key = Some(derive_key(next, &salt)?);
        self.persist()
    }

    // ── internals ─────────────────────────────────────────────────────────

    fn read_envelope(&self) -> Result<VaultFile, String> {
        let raw = std::fs::read(&self.path).map_err(|e| format!("cannot read vault: {e}"))?;
        let file: VaultFile = serde_json::from_slice(&raw)
            .map_err(|_| "vault file corrupted (envelope)".to_string())?;
        if file.version != VAULT_VERSION {
            return Err(format!("unsupported vault version {}", file.version));
        }
        Ok(file)
    }

    fn decrypt_into_memory(&mut self, file: &VaultFile) -> Result<(), String> {
        let key = self.key.as_ref().ok_or("no key")?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let nonce_bytes = B64
            .decode(&file.nonce_b64)
            .map_err(|_| "vault file corrupted (nonce)".to_string())?;
        if nonce_bytes.len() != 12 {
            return Err("vault file corrupted (nonce length)".into());
        }
        let ciphertext = B64
            .decode(&file.ciphertext_b64)
            .map_err(|_| "vault file corrupted (ciphertext)".to_string())?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|_| "decryption failed".to_string())?;
        let mut plaintext = Zeroizing::new(plaintext);
        let payload: VaultPayload = serde_json::from_slice(&plaintext)
            .map_err(|_| "vault file corrupted (payload)".to_string())?;
        plaintext.zeroize();
        self.payload = Some(payload);
        Ok(())
    }

    fn persist(&self) -> Result<(), String> {
        let key = self.key.as_ref().ok_or("Vault is locked")?;
        let payload = self.payload.as_ref().ok_or("Vault is locked")?;
        let mode = self.mode.ok_or("Vault has no mode")?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let plaintext = Zeroizing::new(
            serde_json::to_vec(payload).map_err(|e| format!("serialize vault: {e}"))?,
        );
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_slice())
            .map_err(|_| "encryption failed".to_string())?;
        let file = VaultFile {
            version: VAULT_VERSION,
            mode,
            salt_b64: self.salt.map(|s| B64.encode(s)),
            nonce_b64: B64.encode(nonce),
            ciphertext_b64: B64.encode(&ciphertext),
        };
        let json = serde_json::to_vec_pretty(&file).map_err(|e| format!("serialize vault: {e}"))?;
        atomic_write(&self.path, &json).map_err(|e| format!("write vault: {e}"))
    }

    fn keyring_key(&self, create_if_missing: bool) -> Result<Zeroizing<[u8; 32]>, String> {
        if let Some(k) = self.keyring_override {
            return Ok(Zeroizing::new(k));
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| format!("keyring unavailable: {e}"))?;
        match entry.get_password() {
            Ok(b64) => {
                let bytes = B64
                    .decode(b64.trim())
                    .map_err(|_| "keyring entry corrupted".to_string())?;
                let key: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| "keyring entry corrupted (length)".to_string())?;
                Ok(Zeroizing::new(key))
            }
            Err(keyring::Error::NoEntry) if create_if_missing => {
                let mut key = [0u8; 32];
                OsRng.fill_bytes(&mut key);
                entry
                    .set_password(&B64.encode(key))
                    .map_err(|e| format!("cannot store vault key in OS credential store: {e}"))?;
                Ok(Zeroizing::new(key))
            }
            Err(e) => Err(format!(
                "cannot read vault key from OS credential store: {e}"
            )),
        }
    }
}

fn validate_passphrase(pass: &str) -> Result<(), String> {
    if pass.chars().count() < 10 {
        return Err("Passphrase must be at least 10 characters".into());
    }
    Ok(())
}

fn derive_key(passphrase: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    let params = argon2::Params::new(argon_m_kib(), ARGON_T, ARGON_P, Some(32))
        .map_err(|e| format!("argon2 params: {e}"))?;
    let argon = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, out.as_mut())
        .map_err(|e| format!("argon2: {e}"))?;
    Ok(out)
}

#[cfg(not(test))]
fn argon_m_kib() -> u32 {
    64 * 1024
}

/// Tests use light Argon2 params — same code path, fast suite.
#[cfg(test)]
fn argon_m_kib() -> u32 {
    8 * 1024
}

/// Write via temp file + rename so a crash can never leave a half-written vault.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows: rename fails if target exists — replace explicitly.
            std::fs::remove_file(path)?;
            std::fs::rename(&tmp, path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_vault_path(dir: &TempDir) -> PathBuf {
        dir.path().join("vault.bin")
    }

    #[test]
    fn keyring_mode_roundtrip() {
        let dir = TempDir::new().unwrap();
        let key = [7u8; 32];
        let mut v = Vault::with_keyring_override(tmp_vault_path(&dir), key);
        assert_eq!(v.state(), VaultState::Uninitialized);
        v.initialize(VaultMode::Keyring, None).unwrap();
        v.insert_token("agent-1", "tk_secret_a").unwrap();
        v.insert_token("agent-2", "tk_secret_b").unwrap();
        assert_eq!(v.token_count(), 2);

        // Fresh instance, same key → auto-unlock and read back.
        let mut v2 = Vault::with_keyring_override(tmp_vault_path(&dir), key);
        assert_eq!(v2.open().unwrap(), VaultState::Unlocked);
        assert_eq!(v2.token_for("agent-1").unwrap().as_str(), "tk_secret_a");
        v2.remove_token("agent-1").unwrap();
        assert!(v2.token_for("agent-1").is_none());
        assert_eq!(v2.token_count(), 1);
    }

    #[test]
    fn keyring_mode_wrong_key_fails_closed() {
        let dir = TempDir::new().unwrap();
        let mut v = Vault::with_keyring_override(tmp_vault_path(&dir), [1u8; 32]);
        v.initialize(VaultMode::Keyring, None).unwrap();
        v.insert_token("a", "tk_x").unwrap();

        let mut v2 = Vault::with_keyring_override(tmp_vault_path(&dir), [2u8; 32]);
        let state = v2.open().unwrap();
        // Wrong key → decrypt fails → treated as keyring-unavailable, never a panic.
        assert_eq!(state, VaultState::KeyringUnavailable);
        assert!(v2.token_for("a").is_none());
    }

    #[test]
    fn passphrase_mode_roundtrip_and_wrong_pass() {
        let dir = TempDir::new().unwrap();
        let mut v = Vault::new(tmp_vault_path(&dir));
        v.initialize(VaultMode::Passphrase, Some("correct horse battery"))
            .unwrap();
        v.insert_token("a", "tk_secret").unwrap();

        let mut v2 = Vault::new(tmp_vault_path(&dir));
        assert_eq!(v2.open().unwrap(), VaultState::Locked);
        assert!(v2.unlock("wrong passphrase!!").is_err());
        assert_eq!(v2.state(), VaultState::Locked);
        v2.unlock("correct horse battery").unwrap();
        assert_eq!(v2.state(), VaultState::Unlocked);
        assert_eq!(v2.token_for("a").unwrap().as_str(), "tk_secret");

        v2.lock();
        assert_eq!(v2.state(), VaultState::Locked);
        assert!(v2.token_for("a").is_none());
    }

    #[test]
    fn passphrase_change_rotates_salt_and_key() {
        let dir = TempDir::new().unwrap();
        let mut v = Vault::new(tmp_vault_path(&dir));
        v.initialize(VaultMode::Passphrase, Some("first-passphrase"))
            .unwrap();
        v.insert_token("a", "tk_secret").unwrap();
        let salt_before = v.salt;
        assert!(v.change_passphrase("wrong", "next-passphrase").is_err());
        v.change_passphrase("first-passphrase", "next-passphrase")
            .unwrap();
        assert_ne!(v.salt, salt_before);

        let mut v2 = Vault::new(tmp_vault_path(&dir));
        v2.open().unwrap();
        assert!(v2.unlock("first-passphrase").is_err());
        v2.unlock("next-passphrase").unwrap();
        assert_eq!(v2.token_for("a").unwrap().as_str(), "tk_secret");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let dir = TempDir::new().unwrap();
        let key = [9u8; 32];
        let mut v = Vault::with_keyring_override(tmp_vault_path(&dir), key);
        v.initialize(VaultMode::Keyring, None).unwrap();
        v.insert_token("a", "tk_secret").unwrap();

        // Flip one ciphertext byte on disk.
        let raw = std::fs::read(tmp_vault_path(&dir)).unwrap();
        let mut file: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        let ct = file["ciphertext_b64"].as_str().unwrap().to_string();
        let mut bytes = B64.decode(ct).unwrap();
        bytes[0] ^= 0xff;
        file["ciphertext_b64"] = serde_json::Value::String(B64.encode(&bytes));
        std::fs::write(tmp_vault_path(&dir), serde_json::to_vec(&file).unwrap()).unwrap();

        let mut v2 = Vault::with_keyring_override(tmp_vault_path(&dir), key);
        // GCM auth tag catches the flip; vault refuses to open rather than
        // returning corrupted secrets.
        assert_eq!(v2.open().unwrap(), VaultState::KeyringUnavailable);
        assert!(v2.token_for("a").is_none());
    }

    #[test]
    fn corrupted_envelope_is_a_clean_error() {
        let dir = TempDir::new().unwrap();
        let path = tmp_vault_path(&dir);
        std::fs::write(&path, b"not json at all").unwrap();
        let mut v = Vault::new(path);
        assert!(v.open().is_err());
    }

    #[test]
    fn short_passphrase_rejected() {
        let dir = TempDir::new().unwrap();
        let mut v = Vault::new(tmp_vault_path(&dir));
        assert!(v.initialize(VaultMode::Passphrase, Some("short")).is_err());
    }
}
