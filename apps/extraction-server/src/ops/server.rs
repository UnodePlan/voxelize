use std::{io, sync::Arc};

use actix_web::{web, App, HttpServer};

use super::{
    audit::StderrAuditSink,
    config::OpsConfig,
    http::{configure_ops, OpsHttpState},
    postgres::PgOpsRepository,
};

pub async fn run_ops(config: OpsConfig) -> io::Result<()> {
    let repository = Arc::new(
        PgOpsRepository::connect(config.database_url(), config.query_timeout())
            .await
            .map_err(|error| io::Error::other(format!("运维只读仓储启动失败: {error:?}")))?,
    );
    let state = web::Data::new(OpsHttpState::from_hash(
        repository,
        config.token_hash(),
        config.query_timeout(),
        config.requests_per_minute(),
        Arc::new(StderrAuditSink),
    ));
    HttpServer::new(move || App::new().app_data(state.clone()).configure(configure_ops))
        .workers(2)
        .bind(config.bind_address())?
        .run()
        .await
}
