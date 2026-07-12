use signinwithethereum::generate_nonce;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthRandomError;

pub trait AuthRandom: Send + Sync {
    fn nonce(&self) -> Result<String, AuthRandomError>;
    fn session_token(&self) -> Result<String, AuthRandomError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SecureAuthRandom;

impl AuthRandom for SecureAuthRandom {
    fn nonce(&self) -> Result<String, AuthRandomError> {
        Ok(generate_nonce())
    }

    fn session_token(&self) -> Result<String, AuthRandomError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| AuthRandomError)?;
        Ok(hex::encode(bytes))
    }
}
