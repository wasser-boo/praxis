use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;

pub struct Encryption {
    cipher: Aes256Gcm,
}

impl Encryption {
    pub fn new(password: &str, salt: &[u8]) -> anyhow::Result<Self> {
        let mut key = [0u8; 32];
        Argon2::default()
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|e| anyhow::anyhow!("Argon2 error: {}", e))?;

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("Cipher error: {}", e))?;

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
    fn test_encrypt_decrypt() {
        let enc = Encryption::new("testpassword", b"testsalt12345678").unwrap();
        let encrypted = enc.encrypt("Hello, World!").unwrap();
        let decrypted = enc.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "Hello, World!");
    }

    #[test]
    fn test_different_passwords() {
        let enc1 = Encryption::new("password1", b"testsalt12345678").unwrap();
        let enc2 = Encryption::new("password2", b"testsalt12345678").unwrap();
        let encrypted = enc1.encrypt("secret").unwrap();
        assert!(enc2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_invalid_data() {
        let enc = Encryption::new("password", b"testsalt12345678").unwrap();
        assert!(enc.decrypt(&[1, 2, 3]).is_err());
    }
}
