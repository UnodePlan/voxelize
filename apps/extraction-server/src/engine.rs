use std::{io, sync::Arc};

use voxelize::{HttpConfig, Server, Voxelize};

use crate::{
    auth::SessionConnectionAuthenticator, bootstrap::Application, configure_api, ServerConfig,
};

pub(crate) async fn run(config: ServerConfig, application: Application) -> io::Result<()> {
    let connection_auth = Arc::new(SessionConnectionAuthenticator::new(
        application.auth.clone(),
    ));
    let route_state = application.state.clone();
    let http = HttpConfig::authenticated_arc(connection_auth)
        .allowed_origins([config.public_origin().to_owned()])
        .max_http_payload_size(16 * 1024)
        .configure_routes(move |routes| {
            routes
                .app_data(route_state.clone())
                .configure(configure_api);
        });
    let bind = config.bind_address();
    let server = Server::new()
        .addr(&bind.ip().to_string())
        .port(bind.port())
        .http_config(http)
        .build();
    Voxelize::run(server).await
}
