use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};
use signinwithethereum::Message;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    contracts::ErrorCode,
    ports::{
        AuthRepository, AuthRepositoryError, Clock, LoginCommand, NewNonce, SessionRecord,
        WarehouseSnapshot,
    },
};

use super::{
    nonce_pruner::NoncePruner, AuthConfig, AuthRandom, SignatureVerificationError,
    SignatureVerifier,
};

#[derive(Clone)]
pub struct AuthService {
    repository: Arc<dyn AuthRepository>,
    verifier: Arc<dyn SignatureVerifier>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn AuthRandom>,
    config: AuthConfig,
    nonce_pruner: NoncePruner,
}

impl AuthService {
    pub fn new(
        repository: Arc<dyn AuthRepository>,
        verifier: Arc<dyn SignatureVerifier>,
        clock: Arc<dyn Clock>,
        random: Arc<dyn AuthRandom>,
        config: AuthConfig,
    ) -> Self {
        Self {
            repository,
            verifier,
            clock,
            random,
            config,
            nonce_pruner: NoncePruner::default(),
        }
    }

    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    pub async fn issue_nonce(&self) -> Result<IssuedNonce, AuthError> {
        let nonce = self
            .random
            .nonce()
            .map_err(|_| AuthError::ServiceUnavailable)?;
        let now = self.now();
        let expires_at = now + self.config.nonce_ttl;
        self.nonce_pruner.after_issue(self.repository.clone(), now);
        self.repository
            .insert_nonce(NewNonce {
                id: Uuid::new_v4(),
                nonce_hash: hash_secret(&nonce),
                domain: self.config.domain.clone(),
                uri: self.config.uri.clone(),
                created_at: now,
                expires_at,
            })
            .await
            .map_err(AuthError::from_repository)?;
        Ok(IssuedNonce { nonce, expires_at })
    }

    pub async fn verify_and_create_session(
        &self,
        raw_message: &str,
        raw_signature: &str,
    ) -> Result<CreatedSession, AuthError> {
        let message: Message = raw_message.parse().map_err(|_| AuthError::InvalidSiwe)?;
        let signature = decode_signature(raw_signature)?;
        let nonce_hash = hash_secret(&message.nonce);
        let stored_nonce = self
            .repository
            .find_nonce(nonce_hash)
            .await
            .map_err(AuthError::from_repository)?
            .ok_or(AuthError::NonceInvalid)?;
        let now = self.now();
        if stored_nonce.consumed_at.is_some()
            || stored_nonce.expires_at <= now
            || stored_nonce.domain != self.config.domain
            || stored_nonce.uri != self.config.uri
        {
            return Err(AuthError::NonceInvalid);
        }

        self.verifier
            .verify(&message, &signature, &message.nonce, now)
            .await
            .map_err(AuthError::from_signature)?;

        let completion_now = self.now().max(now);
        let issued_at = *message.issued_at.as_ref();
        let expiration = message
            .expiration_time
            .as_ref()
            .map(|value| *value.as_ref())
            .ok_or(AuthError::InvalidSiwe)?;
        let authentication_expires_at = stored_nonce
            .expires_at
            .min(expiration)
            .min(issued_at + self.config.max_message_age);
        if stored_nonce.expires_at <= completion_now {
            return Err(AuthError::NonceInvalid);
        }
        if authentication_expires_at <= completion_now || !message.valid_at(&completion_now) {
            return Err(AuthError::InvalidSiwe);
        }

        let token = self
            .random
            .session_token()
            .map_err(|_| AuthError::ServiceUnavailable)?;
        let result = self
            .repository
            .complete_login(LoginCommand {
                nonce_hash,
                nonce_domain: stored_nonce.domain,
                nonce_uri: stored_nonce.uri,
                proposed_account_id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                session_token_hash: hash_secret(&token),
                chain_id: message.chain_id as i64,
                address: message.address,
                now: completion_now,
                authentication_expires_at,
                session_expires_at: completion_now + self.config.session_ttl,
                session_idle_expires_at: completion_now + self.config.session_idle_ttl,
            })
            .await
            .map_err(AuthError::from_repository)?;

        Ok(CreatedSession {
            token,
            session: result.session.into(),
            revoked_session_ids: result.revoked_session_ids,
        })
    }

    pub async fn authenticate_token(&self, token: &str) -> Result<AuthSession, AuthError> {
        let now = self.now();
        let record = self
            .repository
            .find_active_session(validated_token_hash(token)?, now)
            .await
            .map_err(AuthError::from_repository)?
            .ok_or(AuthError::Required)?;
        if record.valid_until() <= self.now() {
            return Err(AuthError::Required);
        }
        Ok(record.into())
    }

    pub async fn inspect_token(&self, token: &str) -> Result<AuthSession, AuthError> {
        let record = self
            .repository
            .inspect_active_session(validated_token_hash(token)?, self.now())
            .await
            .map_err(AuthError::from_repository)?
            .ok_or(AuthError::Required)?;
        if record.valid_until() <= self.now() {
            return Err(AuthError::Required);
        }
        Ok(record.into())
    }

    pub async fn revoke_token(&self, token: Option<&str>) -> Result<Option<Uuid>, AuthError> {
        let Some(token) = token.filter(|value| value.len() == 64) else {
            return Ok(None);
        };
        self.repository
            .revoke_session(hash_secret(token), self.now())
            .await
            .map_err(AuthError::from_repository)
    }

    pub async fn warehouse(&self, account_id: Uuid) -> Result<WarehouseSnapshot, AuthError> {
        self.repository
            .warehouse(account_id)
            .await
            .map_err(AuthError::from_repository)
    }

    fn now(&self) -> OffsetDateTime {
        self.clock.utc_now().into()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedNonce {
    pub nonce: String,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedSession {
    pub token: String,
    pub session: AuthSession,
    pub revoked_session_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthSession {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub chain_id: i64,
    pub address: [u8; 20],
    pub expires_at: OffsetDateTime,
    pub idle_expires_at: OffsetDateTime,
}

impl AuthSession {
    pub fn valid_until(&self) -> OffsetDateTime {
        self.expires_at.min(self.idle_expires_at)
    }
}

impl From<SessionRecord> for AuthSession {
    fn from(value: SessionRecord) -> Self {
        Self {
            session_id: value.session_id,
            account_id: value.account_id,
            chain_id: value.chain_id,
            address: value.address,
            expires_at: value.expires_at,
            idle_expires_at: value.idle_expires_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub address: String,
    pub chain_id: i64,
}

impl From<&AuthSession> for SessionView {
    fn from(value: &AuthSession) -> Self {
        Self {
            address: format!("0x{}", hex::encode(value.address)),
            chain_id: value.chain_id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthError {
    Required,
    WrongNetwork,
    InvalidSiwe,
    NonceInvalid,
    ServiceUnavailable,
}

impl AuthError {
    pub fn code(self) -> ErrorCode {
        match self {
            Self::Required => ErrorCode::AuthRequired,
            Self::WrongNetwork => ErrorCode::AuthWrongNetwork,
            Self::InvalidSiwe => ErrorCode::AuthInvalidSiwe,
            Self::NonceInvalid => ErrorCode::AuthNonceInvalid,
            Self::ServiceUnavailable => ErrorCode::ServiceUnavailable,
        }
    }

    fn from_repository(error: AuthRepositoryError) -> Self {
        match error {
            AuthRepositoryError::NonceInvalid => Self::NonceInvalid,
            AuthRepositoryError::AuthenticationExpired => Self::InvalidSiwe,
            AuthRepositoryError::Unavailable => Self::ServiceUnavailable,
        }
    }

    fn from_signature(error: SignatureVerificationError) -> Self {
        match error {
            SignatureVerificationError::Invalid => Self::InvalidSiwe,
            SignatureVerificationError::WrongNetwork => Self::WrongNetwork,
            SignatureVerificationError::Unavailable => Self::ServiceUnavailable,
        }
    }
}

fn decode_signature(value: &str) -> Result<Vec<u8>, AuthError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if value.is_empty() || value.len() > 16_384 {
        return Err(AuthError::InvalidSiwe);
    }
    hex::decode(value).map_err(|_| AuthError::InvalidSiwe)
}

fn validated_token_hash(token: &str) -> Result<[u8; 32], AuthError> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AuthError::Required);
    }
    Ok(hash_secret(token))
}

pub(crate) fn hash_secret(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}
