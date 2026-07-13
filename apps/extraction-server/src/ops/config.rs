use std::{env, error::Error, fmt, net::SocketAddr, time::Duration};

use sha2::{Digest, Sha256};

const ENABLED_ENV: &str = "EXTRACTION_OPS_ENABLED";
const BIND_ENV: &str = "EXTRACTION_OPS_BIND";
const DATABASE_ENV: &str = "EXTRACTION_OPS_DATABASE_URL";
const TOKEN_ENV: &str = "EXTRACTION_OPS_TOKEN";
const TIMEOUT_ENV: &str = "EXTRACTION_OPS_QUERY_TIMEOUT_MS";
const RATE_ENV: &str = "EXTRACTION_OPS_REQUESTS_PER_MINUTE";
const DEFAULT_BIND: &str = "127.0.0.1:4101";
const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_RATE: u32 = 60;
const MIN_TOKEN_BYTES: usize = 32;

pub struct OpsConfig {
    bind_address: SocketAddr,
    database_url: String,
    token_hash: [u8; 32],
    query_timeout: Duration,
    requests_per_minute: u32,
}

impl OpsConfig {
    pub fn from_env() -> Result<Option<Self>, OpsConfigError> {
        if !env_bool(ENABLED_ENV, false)? {
            return Ok(None);
        }
        let bind_address: SocketAddr = env::var(BIND_ENV)
            .unwrap_or_else(|_| DEFAULT_BIND.to_owned())
            .parse()
            .map_err(|_| OpsConfigError::InvalidBind)?;
        if !bind_address.ip().is_loopback() {
            return Err(OpsConfigError::NonLoopbackBind);
        }
        let database_url = required_env(DATABASE_ENV)?;
        let token = required_env(TOKEN_ENV)?;
        if token.len() < MIN_TOKEN_BYTES {
            return Err(OpsConfigError::WeakToken);
        }
        let timeout_ms = env_u64(TIMEOUT_ENV, DEFAULT_TIMEOUT_MS, 50, 5_000)?;
        let requests_per_minute = env_u32(RATE_ENV, DEFAULT_RATE, 1, 1_000)?;
        Ok(Some(Self {
            bind_address,
            database_url,
            token_hash: hash_token(&token),
            query_timeout: Duration::from_millis(timeout_ms),
            requests_per_minute,
        }))
    }

    pub(crate) fn bind_address(&self) -> SocketAddr {
        self.bind_address
    }

    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }

    pub(crate) fn token_hash(&self) -> [u8; 32] {
        self.token_hash
    }

    pub(crate) fn query_timeout(&self) -> Duration {
        self.query_timeout
    }

    pub(crate) fn requests_per_minute(&self) -> u32 {
        self.requests_per_minute
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpsConfigError {
    InvalidBind,
    InvalidBoolean(&'static str),
    InvalidNumber(&'static str),
    MissingVariable(&'static str),
    NonLoopbackBind,
    WeakToken,
}

impl fmt::Display for OpsConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBind => formatter.write_str("EXTRACTION_OPS_BIND 不是有效监听地址"),
            Self::InvalidBoolean(name) => write!(formatter, "{name} 必须是 true 或 false"),
            Self::InvalidNumber(name) => write!(formatter, "{name} 超出允许范围"),
            Self::MissingVariable(name) => write!(formatter, "缺少必需环境变量 {name}"),
            Self::NonLoopbackBind => formatter.write_str("运维监听器只允许绑定 loopback 地址"),
            Self::WeakToken => formatter.write_str("EXTRACTION_OPS_TOKEN 至少需要 32 字节"),
        }
    }
}

impl Error for OpsConfigError {}

pub(crate) fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn required_env(name: &'static str) -> Result<String, OpsConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(OpsConfigError::MissingVariable(name))
}

fn env_bool(name: &'static str, default: bool) -> Result<bool, OpsConfigError> {
    match env::var(name) {
        Ok(value) if value == "true" => Ok(true),
        Ok(value) if value == "false" => Ok(false),
        Ok(_) => Err(OpsConfigError::InvalidBoolean(name)),
        Err(_) => Ok(default),
    }
}

fn env_u64(name: &'static str, default: u64, min: u64, max: u64) -> Result<u64, OpsConfigError> {
    let value = env::var(name)
        .map(|value| value.parse().ok())
        .unwrap_or(Some(default))
        .ok_or(OpsConfigError::InvalidNumber(name))?;
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or(OpsConfigError::InvalidNumber(name))
}

fn env_u32(name: &'static str, default: u32, min: u32, max: u32) -> Result<u32, OpsConfigError> {
    let value = env::var(name)
        .map(|value| value.parse().ok())
        .unwrap_or(Some(default))
        .ok_or(OpsConfigError::InvalidNumber(name))?;
    (min..=max)
        .contains(&value)
        .then_some(value)
        .ok_or(OpsConfigError::InvalidNumber(name))
}
