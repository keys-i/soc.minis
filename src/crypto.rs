use std::{env, fs, path::Path};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::error::{IoContext, Result, invalid};

#[derive(Clone)]
pub struct SecretBox(Aes256Gcm);

impl SecretBox {
    pub fn load(data: &Path) -> Result<Self> {
        let key_path = data.join("state.key");
        let key = match env::var("SOC_STATE_KEY") {
            Ok(value) => URL_SAFE_NO_PAD
                .decode(value)
                .map_err(|_| invalid("SOC_STATE_KEY must be URL-safe base64"))?,
            Err(_) if key_path.is_file() && !key_path.is_symlink() => {
                fs::read(&key_path).at_path("Cannot read", &key_path)?
            }
            Err(_) => {
                let mut key = vec![0_u8; 32];
                getrandom::fill(&mut key)
                    .map_err(|error| invalid(format!("Cannot create a state key: {error}")))?;
                let mut options = fs::OpenOptions::new();
                options.create_new(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                use std::io::Write;
                options
                    .open(&key_path)
                    .at_path("Cannot create", &key_path)?
                    .write_all(&key)
                    .at_path("Cannot write", &key_path)?;
                key
            }
        };
        if key.len() != 32 {
            return Err(invalid("SOC_STATE_KEY must decode to exactly 32 bytes"));
        }
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| invalid("SOC_STATE_KEY must decode to exactly 32 bytes"))?;
        Ok(Self(cipher))
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0_u8; 12];
        getrandom::fill(&mut nonce)
            .map_err(|error| invalid(format!("Cannot create token nonce: {error}")))?;
        let nonce = Nonce::try_from(nonce.as_slice())
            .map_err(|_| invalid("Cannot prepare the token nonce"))?;
        let encrypted = self
            .0
            .encrypt(&nonce, plaintext)
            .map_err(|_| invalid("Cannot encrypt the saved Microsoft session"))?;
        Ok([nonce.as_slice(), encrypted.as_slice()].concat())
    }

    pub fn open(&self, encrypted: &[u8]) -> Result<Vec<u8>> {
        if encrypted.len() < 28 {
            return Err(invalid("The saved Microsoft session is damaged"));
        }
        let nonce = Nonce::try_from(&encrypted[..12])
            .map_err(|_| invalid("The saved Microsoft session has an invalid nonce"))?;
        self.0
            .decrypt(&nonce, &encrypted[12..])
            .map_err(|_| invalid("The saved Microsoft session cannot be decrypted"))
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn seals_tokens_with_a_fresh_nonce() {
        let root = std::env::temp_dir().join(format!("soc-crypto-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("state directory");
        let secrets = SecretBox::load(&root).expect("state key");
        let first = secrets.seal(b"refresh token").expect("encrypted token");
        let second = secrets.seal(b"refresh token").expect("encrypted token");
        assert_ne!(first, second);
        assert_eq!(
            secrets.open(&first).expect("decrypted token"),
            b"refresh token"
        );
        fs::remove_dir_all(root).expect("clean state");
    }
}
