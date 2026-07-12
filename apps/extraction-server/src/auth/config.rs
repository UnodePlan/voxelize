use std::time::Duration as StdDuration;

use time::Duration;

pub const MAINNET_CHAIN_ID: i64 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthConfig {
    pub domain: String,
    pub uri: String,
    pub cookie_secure: bool,
    pub nonce_ttl: Duration,
    pub session_ttl: Duration,
    pub session_idle_ttl: Duration,
    pub max_message_age: Duration,
    pub clock_skew: Duration,
    pub rpc_url: Option<String>,
    pub rpc_timeout: StdDuration,
    pub rpc_concurrency: usize,
}

impl AuthConfig {
    pub fn local(domain: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            uri: uri.into(),
            cookie_secure: false,
            nonce_ttl: Duration::minutes(5),
            session_ttl: Duration::days(7),
            session_idle_ttl: Duration::days(1),
            max_message_age: Duration::minutes(5),
            clock_skew: Duration::seconds(30),
            rpc_url: None,
            rpc_timeout: StdDuration::from_secs(3),
            rpc_concurrency: 8,
        }
    }

    pub fn cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-voxel-extraction-session"
        } else {
            "voxel-extraction-session"
        }
    }
}
