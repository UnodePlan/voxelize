use std::{io, sync::Arc};

use uuid::Uuid;
use voxelize::{ConnectionPrincipal, HttpConfig, Server, Voxelize};

use crate::{
    auth::SessionConnectionAuthenticator, bootstrap::Application, configure_api,
    engine_connection_observer::MatchConnectionObserver, ServerConfig,
};

pub(crate) async fn run(config: ServerConfig, application: Application) -> io::Result<()> {
    let _matchmaking_process_lock = application.matchmaking_process_lock;
    let connection_auth = Arc::new(SessionConnectionAuthenticator::new(
        application.auth.clone(),
    ));
    let route_state = application.state.clone();
    let connection_observer = MatchConnectionObserver::new(&application.matchmaking);
    let http = HttpConfig::authenticated_arc(connection_auth)
        .allowed_origins([config.public_origin().to_owned()])
        .max_http_payload_size(16 * 1024)
        .configure_routes(move |routes| {
            routes
                .app_data(route_state.clone())
                .configure(configure_api);
        });
    let bind = config.bind_address();
    let matchmaking_for_ids = Arc::downgrade(&application.matchmaking);
    let server = Server::new()
        .addr(&bind.ip().to_string())
        .port(bind.port())
        .debug(false)
        .registry(application.engine_catalog.blocks())
        .http_config(http)
        .connection_lifecycle_observer(connection_observer)
        .authenticated_client_id_resolver(
            move |world_name: &str, principal: &ConnectionPrincipal| {
                let service = matchmaking_for_ids.upgrade()?;
                let account_id = match Uuid::parse_str(&principal.account_id) {
                    Ok(account_id) => account_id,
                    Err(_) => {
                        service.fail_closed();
                        return None;
                    }
                };
                service.public_player_id_for(world_name, account_id)
            },
        )
        .build();
    Voxelize::run(server).await
}
