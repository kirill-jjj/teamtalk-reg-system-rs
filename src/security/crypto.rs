use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::Rng;

const ENC_PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct EncryptionService {
    cipher: Aes256Gcm,
}

impl EncryptionService {
    pub fn new_from_base64_key(encoded_key: &str) -> Result<Self> {
        let key_bytes = STANDARD
            .decode(encoded_key.trim())
            .map_err(|e| anyhow::anyhow!("invalid base64 encryption key: {e}"))?;
        if key_bytes.len() != 32 {
            bail!(
                "invalid encryption key length: expected 32 bytes after base64 decode, got {}",
                key_bytes.len()
            );
        }
        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .map_err(|e| anyhow::anyhow!("invalid key: {e}"))?;
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::try_from(&nonce_bytes[..])?;
        let encrypted = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

        let mut payload = Vec::with_capacity(NONCE_LEN + encrypted.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&encrypted);
        Ok(format!("{ENC_PREFIX}{}", STANDARD.encode(payload)))
    }

    pub fn decrypt(&self, encoded: &str) -> Result<String> {
        let Some(raw_payload) = encoded.strip_prefix(ENC_PREFIX) else {
            bail!("payload is not encrypted with current format");
        };
        let payload = STANDARD
            .decode(raw_payload)
            .map_err(|e| anyhow::anyhow!("invalid encrypted payload: {e}"))?;
        if payload.len() <= NONCE_LEN {
            bail!("encrypted payload is too short");
        }

        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes)?;
        let decrypted = self
            .cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;
        String::from_utf8(decrypted).map_err(|e| anyhow::anyhow!("invalid utf-8 plaintext: {e}"))
    }

    pub fn is_encrypted(value: &str) -> bool {
        value.starts_with(ENC_PREFIX)
    }
}

#[cfg(test)]
mod tests {
    use super::EncryptionService;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    fn test_service() -> EncryptionService {
        let key = [7_u8; 32];
        let encoded = STANDARD.encode(key);
        EncryptionService::new_from_base64_key(&encoded).expect("test key should be valid")
    }

    #[test]
    fn round_trip_encrypt_decrypt() {
        let svc = test_service();
        let encrypted = svc.encrypt("secret").expect("encryption should work");
        assert!(EncryptionService::is_encrypted(&encrypted));
        let plain = svc.decrypt(&encrypted).expect("decryption should work");
        assert_eq!(plain, "secret");
    }

    #[test]
    fn decrypt_rejects_non_encrypted_payload() {
        let svc = test_service();
        let err = svc.decrypt("plain").expect_err("must fail for plaintext");
        assert!(err.to_string().contains("not encrypted"));
    }
}
