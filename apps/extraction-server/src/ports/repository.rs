use std::{error::Error, fmt, future::Future, pin::Pin};

pub type RepositoryFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), RepositoryError>> + Send + 'a>>;

pub trait RepositoryProbe: Send + Sync {
    fn check(&self) -> RepositoryFuture<'_>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BootstrapRepositoryProbe;

impl RepositoryProbe for BootstrapRepositoryProbe {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryError {
    message: String,
}

impl RepositoryError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RepositoryError {}
