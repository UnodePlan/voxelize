use std::{
    env,
    error::Error,
    fmt,
    net::{IpAddr, SocketAddr},
};

use actix_web::http::Uri;

use crate::auth::AuthConfig;
use crate::matchmaking::{sanitize_match_capacity, DEV_DEFAULT_MATCH_SIZE, MATCH_SIZE};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:4100";
const DEFAULT_PUBLIC_ORIGIN: &str = "http://127.0.0.1:5173";
const DEFAULT_SIWE_DOMAIN: &str = "127.0.0.1:5173";
const DEFAULT_SIWE_URI: &str = "http://127.0.0.1:5173";
const BIND_ENV: &str = "EXTRACTION_SERVER_BIND";
const DATABASE_ENV: &str = "DATABASE_URL";
const ORIGIN_ENV: &str = "EXTRACTION_PUBLIC_ORIGIN";
const DOMAIN_ENV: &str = "EXTRACTION_SIWE_DOMAIN";
const URI_ENV: &str = "EXTRACTION_SIWE_URI";
const COOKIE_SECURE_ENV: &str = "EXTRACTION_COOKIE_SECURE";
const RPC_ENV: &str = "EXTRACTION_ETHEREUM_RPC_URL";
const AUTH_LOGIN_ENABLED_ENV: &str = "EXTRACTION_AUTH_LOGIN_ENABLED";
const MATCHMAKING_ENABLED_ENV: &str = "EXTRACTION_MATCHMAKING_ENABLED";
const DEV_MATCH_MODE_ENV: &str = "EXTRACTION_DEV_MATCH_MODE";
const DEV_MATCH_SIZE_ENV: &str = "EXTRACTION_DEV_MATCH_SIZE";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    bind_address: SocketAddr,
    database_url: String,
    public_origin: String,
    auth: AuthConfig,
    auth_login_enabled: bool,
    matchmaking_enabled: bool,
    /// 成局人数：生产恒 MATCH_SIZE；DEV mode 下可为 2..=10。
    match_size: usize,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind = env::var(BIND_ENV).unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned());
        let mut config = Self::parse_bind_address(&bind)?;
        config.database_url =
            env::var(DATABASE_ENV).map_err(|_| ConfigError::MissingVariable(DATABASE_ENV))?;
        config.public_origin =
            env::var(ORIGIN_ENV).unwrap_or_else(|_| DEFAULT_PUBLIC_ORIGIN.to_owned());
        config.auth.domain =
            env::var(DOMAIN_ENV).unwrap_or_else(|_| DEFAULT_SIWE_DOMAIN.to_owned());
        config.auth.uri = env::var(URI_ENV).unwrap_or_else(|_| DEFAULT_SIWE_URI.to_owned());
        config.auth.cookie_secure = match env::var(COOKIE_SECURE_ENV) {
            Ok(value) => parse_bool(COOKIE_SECURE_ENV, &value)?,
            Err(_) => config.auth.uri.starts_with("https://"),
        };
        config.auth.rpc_url = env::var(RPC_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty());
        config.auth_login_enabled = env_bool_or(AUTH_LOGIN_ENABLED_ENV, true)?;
        config.matchmaking_enabled = env_bool_or(MATCHMAKING_ENABLED_ENV, true)?;
        config.match_size = resolve_match_size_from_env()?;
        config.validate()?;
        Ok(config)
    }

    pub fn parse_bind_address(value: &str) -> Result<Self, ConfigError> {
        let bind_address = value
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddress(value.to_owned()))?;
        Ok(Self {
            bind_address,
            database_url: "postgres://localhost/voxelize_extraction".to_owned(),
            public_origin: DEFAULT_PUBLIC_ORIGIN.to_owned(),
            auth: AuthConfig::local(DEFAULT_SIWE_DOMAIN, DEFAULT_SIWE_URI),
            auth_login_enabled: true,
            matchmaking_enabled: true,
            match_size: MATCH_SIZE,
        })
    }

    pub fn bind_address(&self) -> SocketAddr {
        self.bind_address
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn public_origin(&self) -> &str {
        &self.public_origin
    }

    pub fn auth(&self) -> &AuthConfig {
        &self.auth
    }

    pub fn auth_login_enabled(&self) -> bool {
        self.auth_login_enabled
    }

    pub fn matchmaking_enabled(&self) -> bool {
        self.matchmaking_enabled
    }

    pub fn match_size(&self) -> usize {
        self.match_size
    }

    pub fn with_database_url(mut self, database_url: impl Into<String>) -> Self {
        self.database_url = database_url.into();
        self
    }

    pub fn with_match_size(mut self, match_size: usize) -> Self {
        self.match_size = sanitize_match_capacity(match_size).unwrap_or(MATCH_SIZE);
        self
    }

    pub fn with_public_auth(mut self, origin: impl Into<String>, auth: AuthConfig) -> Self {
        self.public_origin = origin.into();
        self.auth = auth;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        let uri: Uri = self
            .auth
            .uri
            .parse()
            .map_err(|_| ConfigError::InvalidUri(URI_ENV))?;
        let authority = uri.authority().ok_or(ConfigError::InvalidUri(URI_ENV))?;
        if uri.scheme().is_none() || authority.as_str() != self.auth.domain {
            return Err(ConfigError::InvalidUri(URI_ENV));
        }
        match uri.scheme_str() {
            Some("https") if self.auth.cookie_secure => {}
            Some("http")
                if !self.auth.cookie_secure
                    && uri.host().is_some_and(is_local_development_host) => {}
            _ => return Err(ConfigError::InsecureSessionCookie),
        }

        let origin: Uri = self
            .public_origin
            .parse()
            .map_err(|_| ConfigError::InvalidUri(ORIGIN_ENV))?;
        if origin.scheme().is_none()
            || origin.authority().is_none()
            || origin
                .path_and_query()
                .is_some_and(|value| value.as_str() != "/")
        {
            return Err(ConfigError::InvalidUri(ORIGIN_ENV));
        }
        if origin.scheme_str() != uri.scheme_str() || origin.authority() != Some(authority) {
            return Err(ConfigError::MismatchedPublicOrigin);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidBindAddress(String),
    InvalidBoolean(&'static str),
    InvalidMatchSize(&'static str),
    InsecureSessionCookie,
    InvalidUri(&'static str),
    MismatchedPublicOrigin,
    MissingVariable(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress(value) => {
                write!(formatter, "{BIND_ENV} 不是有效的监听地址: {value}")
            }
            Self::InvalidBoolean(name) => write!(formatter, "{name} 必须是 true 或 false"),
            Self::InvalidMatchSize(name) => {
                write!(
                    formatter,
                    "{name} 必须是 {DEV_DEFAULT_MATCH_SIZE}..={MATCH_SIZE} 的整数"
                )
            }
            Self::InsecureSessionCookie => formatter.write_str(
                "HTTPS 必须启用 Secure Cookie，非 Secure Cookie 仅允许本地 HTTP 开发地址",
            ),
            Self::InvalidUri(name) => write!(formatter, "{name} 不是受支持的绝对 URI"),
            Self::MismatchedPublicOrigin => formatter.write_str(
                "EXTRACTION_PUBLIC_ORIGIN 必须与 EXTRACTION_SIWE_URI 使用相同 scheme 和 authority",
            ),
            Self::MissingVariable(name) => write!(formatter, "缺少必需环境变量 {name}"),
        }
    }
}

impl Error for ConfigError {}

/// DEV 成局人数：仅 `EXTRACTION_DEV_MATCH_MODE=true` 时生效，否则恒为 10。
fn resolve_match_size_from_env() -> Result<usize, ConfigError> {
    let dev_mode = env_bool_or(DEV_MATCH_MODE_ENV, false)?;
    if !dev_mode {
        return Ok(MATCH_SIZE);
    }
    match env::var(DEV_MATCH_SIZE_ENV) {
        Ok(raw) => {
            let parsed = raw
                .trim()
                .parse::<usize>()
                .map_err(|_| ConfigError::InvalidMatchSize(DEV_MATCH_SIZE_ENV))?;
            sanitize_match_capacity(parsed)
                .ok_or(ConfigError::InvalidMatchSize(DEV_MATCH_SIZE_ENV))
        }
        Err(_) => Ok(DEV_DEFAULT_MATCH_SIZE),
    }
}

fn parse_bool(name: &'static str, value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ConfigError::InvalidBoolean(name)),
    }
}

fn env_bool_or(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(name) {
        Ok(value) => parse_bool(name, &value),
        Err(_) => Ok(default),
    }
}

fn is_local_development_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_socket_address() {
        let config = ServerConfig::parse_bind_address("127.0.0.1:4200").unwrap();
        assert_eq!(config.bind_address().port(), 4200);
    }

    #[test]
    fn rejects_an_invalid_socket_address() {
        let error = ServerConfig::parse_bind_address("localhost").unwrap_err();
        assert_eq!(
            error,
            ConfigError::InvalidBindAddress("localhost".to_owned())
        );
    }

    #[test]
    fn rejects_cookie_downgrades_and_public_origin_mismatch() {
        let https_auth = AuthConfig::local("game.example", "https://game.example");
        let https = ServerConfig::parse_bind_address("127.0.0.1:4200")
            .unwrap()
            .with_public_auth("https://game.example", https_auth);
        assert_eq!(https.validate(), Err(ConfigError::InsecureSessionCookie));

        let public_http_auth = AuthConfig::local("game.example", "http://game.example");
        let public_http = ServerConfig::parse_bind_address("127.0.0.1:4200")
            .unwrap()
            .with_public_auth("http://game.example", public_http_auth);
        assert_eq!(
            public_http.validate(),
            Err(ConfigError::InsecureSessionCookie)
        );

        let mut mismatch_auth = AuthConfig::local("game.example", "https://game.example");
        mismatch_auth.cookie_secure = true;
        let mismatch = ServerConfig::parse_bind_address("127.0.0.1:4200")
            .unwrap()
            .with_public_auth("https://other.example", mismatch_auth);
        assert_eq!(
            mismatch.validate(),
            Err(ConfigError::MismatchedPublicOrigin)
        );
    }

    #[test]
    fn allows_secure_https_and_loopback_http_cookie_modes() {
        let mut https_auth = AuthConfig::local("game.example", "https://game.example");
        https_auth.cookie_secure = true;
        let https = ServerConfig::parse_bind_address("127.0.0.1:4200")
            .unwrap()
            .with_public_auth("https://game.example", https_auth);
        assert_eq!(https.validate(), Ok(()));

        let local_auth = AuthConfig::local("127.0.0.1:5173", "http://127.0.0.1:5173");
        let local = ServerConfig::parse_bind_address("127.0.0.1:4200")
            .unwrap()
            .with_public_auth("http://127.0.0.1:5173", local_auth);
        assert_eq!(local.validate(), Ok(()));
    }
}
