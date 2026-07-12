mod common;
mod errors;
mod http;
mod libs;
mod server;
mod types;
pub mod webrtc;
mod world;

use std::sync::Arc;

use actix::{Actor, Addr};
use actix_cors::Cors;
use actix_files::{Files, NamedFile};
use actix_web::{middleware, web, App, HttpResponse, HttpServer, Result};
use hashbrown::HashMap;
use log::info;
use tokio::sync::{mpsc, Mutex};

pub use common::*;
pub use http::*;
pub use libs::*;
pub use server::*;
pub use types::*;
pub use webrtc::signaling::{rtc_candidate, rtc_offer, WebRTCPeers};
pub use webrtc::{create_webrtc_api, datachannel::fragment_message};
pub use world::system_profiler::{
    clear_timing_data_for_world, get_all_world_names, get_timing_summary_for_world, SystemTimer,
    TimedDispatcherBuilder, TimedSystem, WorldTimingContext,
};
pub use world::*;

pub type RtcSenders = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>>;

pub fn create_rtc_senders() -> RtcSenders {
    Arc::new(Mutex::new(HashMap::new()))
}

struct Config {
    serve: String,
}

async fn index(path: web::Data<Config>) -> Result<NamedFile> {
    let path = path.serve.to_owned();
    Ok(NamedFile::open(if path.ends_with("/") {
        path + "index.html"
    } else {
        path + "/index.html"
    })?)
}

async fn info(server: web::Data<Addr<Server>>) -> Result<HttpResponse> {
    let info = server.send(Info).await.unwrap();
    Ok(HttpResponse::Ok().json(info))
}

pub struct Voxelize;

impl Voxelize {
    pub async fn run(mut server: Server) -> std::io::Result<()> {
        if server.http_config.security_mode() == ConnectionSecurityMode::PublicStrict
            && server.secret.is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "public HTTP mode cannot use the legacy shared secret",
            ));
        }
        server.http_config.validate()?;
        server.prepare().await;
        server.preload().await;
        server.started = true;

        let addr = server.addr.to_owned();
        let port = server.port.to_owned();
        let serve = server.serve.to_owned();
        let secret = server.secret.to_owned();
        let http_config = server.http_config.clone();

        let server_addr = server.start();

        if serve.is_empty() {
            info!("Attempting to serve static folder: {}", serve);
        }

        let srv = HttpServer::new(move || {
            let serve = serve.to_owned();
            let http_config = http_config.clone();
            let cors = build_cors(&http_config);
            let handshake_config = HandshakeConfig {
                secret: secret.clone(),
                http: http_config.clone(),
            };
            let route_config = http_config.clone();

            let mut app = App::new()
                .wrap(cors)
                .wrap(middleware::from_fn(strict_origin_guard))
                .app_data(web::PayloadConfig::new(
                    http_config.max_http_payload_size_bytes(),
                ))
                .app_data(
                    web::JsonConfig::default().limit(http_config.max_http_payload_size_bytes()),
                )
                .app_data(web::Data::new(http_config.clone()))
                .app_data(web::Data::new(handshake_config))
                .app_data(web::Data::new(server_addr.clone()))
                .app_data(web::Data::new(Config {
                    serve: serve.to_owned(),
                }))
                .route("/", web::get().to(index))
                .route("/ws/", web::get().to(ws_route))
                .configure(move |config| route_config.apply_routes(config));

            if http_config.exposes_info() {
                app = app.route("/info", web::get().to(info));
            }

            if serve.is_empty() {
                app
            } else {
                app.service(Files::new("/", serve).show_files_listing())
            }
        })
        .bind((addr.to_owned(), port.to_owned()))?;

        info!("Voxelize backend running on http://{}:{}", addr, port);

        srv.run().await
    }
}

fn build_cors(config: &HttpConfig) -> Cors {
    match config.cors() {
        CorsPolicy::Permissive => Cors::permissive(),
        CorsPolicy::AllowList(origins) => origins.iter().fold(
            Cors::default()
                .allowed_methods(["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
                .allow_any_header()
                .block_on_origin_mismatch(true)
                .supports_credentials(),
            |cors, origin| cors.allowed_origin(origin),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use actix_web::{
        http::{header, StatusCode},
        middleware, test, web, App, HttpResponse,
    };

    use super::*;

    async fn counted_route(hits: web::Data<Arc<AtomicUsize>>) -> HttpResponse {
        hits.fetch_add(1, Ordering::SeqCst);
        HttpResponse::Ok().finish()
    }

    fn strict_config() -> HttpConfig {
        HttpConfig::authenticated(|_: ConnectionAuthRequest| async {
            Ok(ConnectionPrincipal::new("account-1", "session-1"))
        })
        .allowed_origins(["https://game.example"])
    }

    #[actix_web::test]
    async fn strict_origin_guard_rejects_before_custom_route() {
        let config = strict_config();
        let hits = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .wrap(build_cors(&config))
                .wrap(middleware::from_fn(strict_origin_guard))
                .app_data(web::Data::new(config))
                .app_data(web::Data::new(hits.clone()))
                .route("/custom", web::get().to(counted_route)),
        )
        .await;

        let denied_requests = [
            test::TestRequest::get().uri("/custom").to_request(),
            test::TestRequest::get()
                .uri("/custom")
                .append_header((header::ORIGIN, "https://game.example"))
                .append_header((header::ORIGIN, "https://game.example"))
                .to_request(),
            test::TestRequest::get()
                .uri("/custom")
                .insert_header((header::ORIGIN, "null"))
                .to_request(),
            test::TestRequest::get()
                .uri("/custom")
                .insert_header((header::ORIGIN, "https://evil.example"))
                .to_request(),
        ];

        for request in denied_requests {
            let response = test::call_service(&app, request).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let allowed = test::TestRequest::get()
            .uri("/custom")
            .insert_header((header::ORIGIN, "https://game.example"))
            .to_request();
        let response = test::call_service(&app, allowed).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[actix_web::test]
    async fn legacy_origin_guard_keeps_requests_compatible() {
        let config = HttpConfig::legacy();
        let hits = Arc::new(AtomicUsize::new(0));
        let app = test::init_service(
            App::new()
                .wrap(build_cors(&config))
                .wrap(middleware::from_fn(strict_origin_guard))
                .app_data(web::Data::new(config))
                .app_data(web::Data::new(hits.clone()))
                .route("/custom", web::get().to(counted_route)),
        )
        .await;

        let missing_origin = test::TestRequest::get().uri("/custom").to_request();
        let response = test::call_service(&app, missing_origin).await;
        assert_eq!(response.status(), StatusCode::OK);

        let arbitrary_origin = test::TestRequest::get()
            .uri("/custom")
            .insert_header((header::ORIGIN, "https://legacy.example"))
            .to_request();
        let response = test::call_service(&app, arbitrary_origin).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }
}
