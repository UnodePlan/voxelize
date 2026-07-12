mod config;
#[cfg(feature = "engine")]
mod connection;
mod cookie;
mod nonce_pruner;
mod random;
mod rate_limit;
mod service;
mod verifier;

pub use config::{AuthConfig, MAINNET_CHAIN_ID};
#[cfg(feature = "engine")]
pub use connection::SessionConnectionAuthenticator;
pub use cookie::{removal_cookie, session_cookie};
pub use random::{AuthRandom, AuthRandomError, SecureAuthRandom};
pub use service::{AuthError, AuthService, AuthSession, CreatedSession, IssuedNonce, SessionView};
pub use verifier::{SignatureVerificationError, SignatureVerifier, SiweSignatureVerifier};

pub(crate) use cookie::cookie_value_from_headers;
pub(crate) use rate_limit::NonceRateLimiter;
