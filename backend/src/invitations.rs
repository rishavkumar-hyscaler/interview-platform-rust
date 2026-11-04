//! Recoverable invitation capabilities. Database stores authenticated ciphertext;
//! the independent encryption key is supplied through private configuration.
use rand::{RngCore, rngs::OsRng};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use uuid::Uuid;

pub struct InvitationCipher(LessSafeKey);
impl InvitationCipher {
    pub fn from_environment() -> anyhow::Result<Self> {
        Self::from_hex(&std::env::var("INVITATION_ENCRYPTION_KEY")?)
    }
    fn from_hex(value: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(value)?;
        anyhow::ensure!(
            bytes.len() == 32,
            "Invitation encryption key must be 32 bytes"
        );
        let key = UnboundKey::new(&aead::CHACHA20_POLY1305, &bytes)
            .map_err(|_| anyhow::anyhow!("Invalid invitation encryption key"))?;
        Ok(Self(LessSafeKey::new(key)))
    }
    fn context(org: Uuid, session: Uuid) -> String {
        format!("interview-invitation:v1:{org}:{session}")
    }
    pub fn seal(&self, token: &str, org: Uuid, session: Uuid) -> anyhow::Result<String> {
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let mut ciphertext = token.as_bytes().to_vec();
        self.0
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::context(org, session).as_bytes()),
                &mut ciphertext,
            )
            .map_err(|_| anyhow::anyhow!("Invitation encryption failed"))?;
        Ok(format!(
            "v1:{}:{}",
            hex::encode(nonce),
            hex::encode(ciphertext)
        ))
    }
    pub fn open(&self, sealed: &str, org: Uuid, session: Uuid) -> anyhow::Result<String> {
        let parts: Vec<_> = sealed.split(':').collect();
        anyhow::ensure!(
            parts.len() == 3 && parts[0] == "v1",
            "Invalid encrypted invitation"
        );
        let nonce: [u8; 12] = hex::decode(parts[1])?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid invitation nonce"))?;
        let mut ciphertext = hex::decode(parts[2])?;
        let token = self
            .0
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::context(org, session).as_bytes()),
                &mut ciphertext,
            )
            .map_err(|_| anyhow::anyhow!("Invitation authentication failed"))?;
        anyhow::ensure!(
            token.len() == 64 && token.iter().all(u8::is_ascii_hexdigit),
            "Invalid invitation token"
        );
        Ok(String::from_utf8(token.to_vec())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tokens_are_recoverable_and_bound_to_their_session() {
        let cipher = InvitationCipher::from_hex(&"ab".repeat(32)).unwrap();
        let org = Uuid::new_v4();
        let session = Uuid::new_v4();
        let token = "12".repeat(32);
        let sealed = cipher.seal(&token, org, session).unwrap();
        assert!(!sealed.contains(&token));
        assert_eq!(cipher.open(&sealed, org, session).unwrap(), token);
        assert_ne!(sealed, cipher.seal(&token, org, session).unwrap());
        assert!(cipher.open(&sealed, Uuid::new_v4(), session).is_err());
        assert!(cipher.open(&sealed, org, Uuid::new_v4()).is_err());
        let wrong_key = InvitationCipher::from_hex(&"cd".repeat(32)).unwrap();
        assert!(wrong_key.open(&sealed, org, session).is_err());
        let mut tampered = sealed.into_bytes();
        let last = tampered.last_mut().unwrap();
        *last = if *last == b'0' { b'1' } else { b'0' };
        assert!(
            cipher
                .open(&String::from_utf8(tampered).unwrap(), org, session)
                .is_err()
        );
    }
}
