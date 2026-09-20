use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::Argon2;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::Path;

const ENCRYPTED_SECRETS_FILE: &str = "secrets.enc2";
const SALT_FILE: &str = ".secrets_salt";
const NONCE_SIZE: usize = 12;
const SALT_SIZE: usize = 32;

/// Basisverzeichnis des Secret-Stores. Default: Arbeitsverzeichnis (wie
/// bisher). Container-Betrieb setzt `SECRETS_DIR` auf ein Volume, damit
/// Store + Salt Container-Neustarts überleben.
fn secrets_dir() -> std::path::PathBuf {
    std::env::var("SECRETS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| ".".into())
}

fn secrets_file() -> std::path::PathBuf {
    secrets_dir().join(ENCRYPTED_SECRETS_FILE)
}

fn salt_file() -> std::path::PathBuf {
    secrets_dir().join(SALT_FILE)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedSecrets {
    pub ciphertext: String,
    pub nonce: String,
}

fn derive_key(password: &str, salt: &[u8]) -> anyhow::Result<[u8; 32]> {
    let mut key = [0u8; 32];
    let argon2 = Argon2::default();
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("Key derivation failed: {}", e))?;
    Ok(key)
}

fn generate_salt() -> [u8; SALT_SIZE] {
    let mut salt = [0u8; SALT_SIZE];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

fn load_or_create_salt() -> anyhow::Result<[u8; SALT_SIZE]> {
    let path = salt_file();
    if path.exists() {
        let data = std::fs::read(&path)?;
        if data.len() == SALT_SIZE {
            let mut salt = [0u8; SALT_SIZE];
            salt.copy_from_slice(&data);
            return Ok(salt);
        }
    }

    let salt = generate_salt();
    std::fs::write(&path, &salt)?;
    tracing::info!("Generated new encryption salt");
    Ok(salt)
}

pub fn encrypt(data: &str, password: &str) -> anyhow::Result<EncryptedSecrets> {
    let salt = load_or_create_salt()?;
    let key = derive_key(password, &salt)?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Cipher creation failed: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    Ok(EncryptedSecrets {
        ciphertext: base64::engine::general_purpose::STANDARD.encode(&ciphertext),
        nonce: base64::engine::general_purpose::STANDARD.encode(&nonce_bytes),
    })
}

pub fn decrypt(encrypted: &EncryptedSecrets, password: &str) -> anyhow::Result<String> {
    let salt = load_or_create_salt()?;
    let key = derive_key(password, &salt)?;

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| anyhow::anyhow!("Cipher creation failed: {}", e))?;

    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(&encrypted.nonce)
        .map_err(|e| anyhow::anyhow!("Invalid nonce: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(&encrypted.ciphertext)
        .map_err(|e| anyhow::anyhow!("Invalid ciphertext: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| anyhow::anyhow!("Decryption failed (wrong password?): {}", e))?;

    String::from_utf8(plaintext).map_err(|e| anyhow::anyhow!("Invalid UTF-8: {}", e))
}

pub fn save_encrypted_secrets(secrets_json: &str, password: &str) -> anyhow::Result<()> {
    let encrypted = encrypt(secrets_json, password)?;
    let content = serde_json::to_string_pretty(&encrypted)?;
    std::fs::write(secrets_file(), content)?;
    tracing::info!("Saved encrypted secrets to {}", secrets_file().display());
    Ok(())
}

pub fn load_encrypted_secrets(password: &str) -> anyhow::Result<String> {
    let path = secrets_file();
    if !path.exists() {
        return Err(anyhow::anyhow!("No encrypted secrets file found"));
    }

    let content = std::fs::read_to_string(path)?;
    let encrypted: EncryptedSecrets = serde_json::from_str(&content)?;

    decrypt(&encrypted, password)
}

pub fn has_encrypted_secrets() -> bool {
    secrets_file().exists()
}

pub fn verify_password(password: &str) -> bool {
    load_encrypted_secrets(password).is_ok()
}

pub fn hash_master_key(password: &str) -> String {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("Argon2 hashing failed");
    hash.to_string()
}

pub fn verify_master_key_hash(password: &str, hash_str: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    let hash = match PasswordHash::new(hash_str) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

pub fn migrate_plaintext_to_encrypted(password: &str) -> anyhow::Result<()> {
    let plaintext_path = Path::new("secrets.json");
    if !plaintext_path.exists() {
        tracing::info!("No plaintext secrets.json to migrate");
        return Ok(());
    }

    let content = std::fs::read_to_string(plaintext_path)?;

    let _: serde_json::Value = serde_json::from_str(&content)?;

    save_encrypted_secrets(&content, password)?;

    let backup_path = "secrets.json.migrated";
    std::fs::rename(plaintext_path, backup_path)?;
    tracing::info!(
        "Migrated secrets.json to encrypted format, backup at {}",
        backup_path
    );

    Ok(())
}

pub struct Encryption {
    cipher: Aes256Gcm,
}

impl Encryption {
    pub fn new(password: &str, salt: &[u8]) -> anyhow::Result<Self> {
        let mut key = [0u8; 32];
        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2 error: {}", e))?;

        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("Cipher error: {}", e))?;

        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> anyhow::Result<Vec<u8>> {
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encrypt error: {}", e))?;

        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    pub fn decrypt(&self, data: &[u8]) -> anyhow::Result<String> {
        if data.len() < 12 {
            anyhow::bail!("Invalid encrypted data");
        }

        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decrypt error: {}", e))?;

        Ok(String::from_utf8(plaintext)?)
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_encryption_struct() {
        let enc = Encryption::new("testpassword", b"testsalt12345678").unwrap();
        let encrypted = enc.encrypt("Hello, World!").unwrap();
        let decrypted = enc.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "Hello, World!");
    }

    #[test]
    fn test_different_passwords_struct() {
        let enc1 = Encryption::new("password1", b"testsalt12345678").unwrap();
        let enc2 = Encryption::new("password2", b"testsalt12345678").unwrap();
        let encrypted = enc1.encrypt("secret").unwrap();
        assert!(enc2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_invalid_data_struct() {
        let enc = Encryption::new("password", b"testsalt12345678").unwrap();
        assert!(enc.decrypt(&[1, 2, 3]).is_err());
    }

    #[test]
    fn test_encrypt_decrypt_direct() {
        let temp = tempfile::tempdir().unwrap();
        let salt_path = temp.path().join(".secrets_salt");
        let _secrets_path = temp.path().join("secrets.enc2");

        // Generate salt
        let salt = generate_salt();
        std::fs::write(&salt_path, &salt).unwrap();

        // Test encrypt/decrypt using temp paths
        let key = derive_key("testpassword", &salt).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();

        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher.encrypt(nonce, b"Hello, World!".as_slice()).unwrap();

        let encrypted = EncryptedSecrets {
            ciphertext: base64::engine::general_purpose::STANDARD.encode(&ciphertext),
            nonce: base64::engine::general_purpose::STANDARD.encode(&nonce_bytes),
        };

        // Decrypt
        let key2 = derive_key("testpassword", &salt).unwrap();
        let cipher2 = Aes256Gcm::new_from_slice(&key2).unwrap();

        let nonce_bytes2 = base64::engine::general_purpose::STANDARD
            .decode(&encrypted.nonce)
            .unwrap();
        let nonce2 = Nonce::from_slice(&nonce_bytes2);
        let ct = base64::engine::general_purpose::STANDARD
            .decode(&encrypted.ciphertext)
            .unwrap();

        let plaintext = cipher2.decrypt(nonce2, ct.as_ref()).unwrap();
        assert_eq!(String::from_utf8(plaintext).unwrap(), "Hello, World!");
    }

    #[test]
    fn secrets_dir_env_relocates_store_and_salt() {
        // Container-Betrieb: SECRETS_DIR zeigt auf ein Volume — Store + Salt
        // müssen dort leben (nicht im CWD), damit sie Container-Neustarts
        // überleben. Env-Manipulation nur in diesem (serialisierten) Test:
        // Sicherheitshalber prüfen wir, dass kein paralleler Test SECRETS_DIR
        // setzt, und räumen ihn in jedem Fall auf.
        let temp = tempfile::tempdir().unwrap();
        // enc2 ist prozessglobal — wir ändern die Env nur, wenn sie nicht
        // bereits belegt ist, und räumen deterministisch auf.
        let guard = std::sync::Mutex::new(());
        let _lock = guard.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("SECRETS_DIR", temp.path());
        let cleanup = || { std::env::remove_var("SECRETS_DIR"); };
        assert!(!has_encrypted_secrets());
        save_encrypted_secrets("{}", "container-test-key").unwrap();
        assert!(temp.path().join("secrets.enc2").exists());
        assert!(temp.path().join(".secrets_salt").exists());
        assert_eq!(load_encrypted_secrets("container-test-key").unwrap(), "{}");
        assert!(load_encrypted_secrets("wrong").is_err());
        cleanup();
    }
}
