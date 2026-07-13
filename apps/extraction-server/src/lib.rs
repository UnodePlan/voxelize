pub mod auth;
mod bootstrap;
mod config;
pub mod contracts;
#[cfg(feature = "engine")]
mod engine;
#[cfg(feature = "engine")]
mod engine_catalog;
#[cfg(feature = "engine")]
mod engine_connection_observer;
#[cfg(feature = "engine")]
mod engine_gameplay;
#[cfg(feature = "engine")]
mod engine_matchmaking;
#[cfg(feature = "engine")]
mod engine_movement;
#[cfg(any(feature = "engine", test))]
mod gameplay;
#[cfg(feature = "engine")]
mod generation;
mod http;
pub mod match_world;
pub mod matchmaking;
pub mod ops;
pub mod persistence;
pub mod ports;

use std::io;

#[cfg(not(feature = "engine"))]
use actix_cors::Cors;
#[cfg(not(feature = "engine"))]
use actix_web::{App, HttpServer};

pub use config::{ConfigError, ServerConfig};
pub use http::{configure_api, AppState, HealthResponse};
#[cfg(feature = "engine")]
pub use voxelize::WorldConfig as EngineWorldConfig;

pub async fn run(config: ServerConfig) -> io::Result<()> {
    config
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let application = bootstrap::build(&config).await?;
    run_application(config, application).await
}

#[cfg(feature = "engine")]
async fn run_application(
    config: ServerConfig,
    application: bootstrap::Application,
) -> io::Result<()> {
    engine::run(config, application).await
}

#[cfg(not(feature = "engine"))]
async fn run_application(
    config: ServerConfig,
    application: bootstrap::Application,
) -> io::Result<()> {
    let origin = config.public_origin().to_owned();
    let _matchmaking_process_lock = application.matchmaking_process_lock;
    let state = application.state;
    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(&origin)
            .allowed_methods(["GET", "POST", "DELETE", "OPTIONS"])
            .allow_any_header()
            .supports_credentials();
        App::new()
            .wrap(cors)
            .app_data(state.clone())
            .configure(configure_api)
    })
    .bind(config.bind_address())?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use crate::auth::AuthConfig;

    use super::*;

    #[actix_web::test]
    async fn run_revalidates_programmatically_constructed_config() {
        let config = ServerConfig::parse_bind_address("127.0.0.1:4200")
            .unwrap()
            .with_public_auth(
                "https://game.example",
                AuthConfig::local("game.example", "https://game.example"),
            );

        let error = run(config)
            .await
            .expect_err("公开 run 入口必须拒绝非 Secure HTTPS Cookie");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
