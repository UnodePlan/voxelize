use std::{env, error::Error, fmt, net::SocketAddr};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:4100";
const BIND_ENV: &str = "EXTRACTION_SERVER_BIND";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerConfig {
    bind_address: SocketAddr,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let value = env::var(BIND_ENV).unwrap_or_else(|_| DEFAULT_BIND_ADDRESS.to_owned());
        Self::parse_bind_address(&value)
    }

    pub fn parse_bind_address(value: &str) -> Result<Self, ConfigError> {
        let bind_address = value
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddress(value.to_owned()))?;
        Ok(Self { bind_address })
    }

    pub fn bind_address(self) -> SocketAddr {
        self.bind_address
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    InvalidBindAddress(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress(value) => {
                write!(formatter, "{BIND_ENV} 不是有效的监听地址: {value}")
            }
        }
    }
}

impl Error for ConfigError {}

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
}
