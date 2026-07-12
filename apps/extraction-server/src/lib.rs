mod config;
pub mod contracts;
mod http;
pub mod ports;

use std::{io, sync::Arc};

use actix_web::{web, App, HttpServer};

pub use config::{ConfigError, ServerConfig};
pub use http::{configure_api, AppState, HealthResponse};
use ports::BootstrapRepositoryProbe;

#[cfg(feature = "engine")]
pub use voxelize::WorldConfig as EngineWorldConfig;

pub async fn run(config: ServerConfig) -> io::Result<()> {
    let manifest = contracts::bundled_manifest()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let state = web::Data::new(AppState::new(Arc::new(BootstrapRepositoryProbe), manifest));

    HttpServer::new(move || App::new().app_data(state.clone()).configure(configure_api))
        .bind(config.bind_address())?
        .run()
        .await
}
