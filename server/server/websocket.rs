use actix::Addr;
use actix_web::{
    http::{header, StatusCode},
    web::{self, Query},
    Error, HttpRequest, HttpResponse, Result,
};
use actix_ws::{AggregatedMessage, CloseCode, CloseReason, ProtocolError};
use futures_util::StreamExt;
use hashbrown::HashMap;
use log::{info, warn};

use crate::{
    decode_message, encode_message, ClientMessage, Connect, ConnectionAuthErrorKind,
    ConnectionAuthRequest, ConnectionPrincipal, Disconnect, HttpConfig, Message, MessageType,
    Server, WsSender,
};

#[derive(Clone)]
pub(crate) struct HandshakeConfig {
    pub(crate) secret: Option<String>,
    pub(crate) http: HttpConfig,
}

pub(crate) async fn ws_route(
    req: HttpRequest,
    body: web::Payload,
    server: web::Data<Addr<Server>>,
    config: web::Data<HandshakeConfig>,
    options: Query<HashMap<String, String>>,
) -> Result<HttpResponse, Error> {
    let mut origin_values = req.headers().get_all(header::ORIGIN).into_iter();
    let origin = origin_values.next().and_then(|value| value.to_str().ok());
    if matches!(config.http.cors(), crate::CorsPolicy::AllowList(_))
        && origin_values.next().is_some()
    {
        return Ok(HttpResponse::Forbidden().finish());
    }
    if !config.http.origin_allowed(origin) {
        return Ok(HttpResponse::Forbidden().finish());
    }

    let principal = if config.http.requires_authentication() {
        // Public mode does not expose the legacy transport trust path.
        if options.contains_key("is_transport") {
            return Ok(HttpResponse::Forbidden().finish());
        }

        let auth_request = ConnectionAuthRequest {
            headers: req.headers().clone(),
            peer_addr: req.peer_addr(),
            path: req.path().to_owned(),
        };

        match tokio::time::timeout(
            config.http.auth_timeout_value(),
            config.http.authenticate(auth_request),
        )
        .await
        {
            Ok(Ok(Some(principal))) if principal.is_valid() => Some(principal),
            Ok(Ok(_)) => return Ok(HttpResponse::Unauthorized().finish()),
            Ok(Err(error)) => {
                let status = match error.kind {
                    ConnectionAuthErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
                    ConnectionAuthErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                };
                return Ok(HttpResponse::build(status).finish());
            }
            Err(_) => return Ok(HttpResponse::ServiceUnavailable().finish()),
        }
    } else {
        validate_legacy_secret(config.secret.as_deref(), &options)?;
        None
    };

    let initial_id = if principal.is_none() {
        requested_legacy_id(&options)
    } else {
        None
    };
    let is_transport = principal.is_none() && options.contains_key("is_transport");

    if is_transport {
        info!("A new transport server has connected.");
    }

    let (response, session, stream) = actix_ws::handle(&req, body)?;
    let max_message_size = config.http.max_ws_message_size_bytes();
    let stream = stream
        .max_frame_size(max_message_size)
        .aggregate_continuations()
        .max_continuation_size(max_message_size);

    actix_web::rt::spawn(handle_ws_connection(
        initial_id,
        principal,
        is_transport,
        session,
        stream,
        server.get_ref().clone(),
        config.http.clone(),
    ));

    Ok(response)
}

fn requested_legacy_id(options: &HashMap<String, String>) -> Option<String> {
    options
        .get("client_id")
        .filter(|id| !id.is_empty())
        .cloned()
}

fn validate_legacy_secret(
    expected: Option<&str>,
    options: &HashMap<String, String>,
) -> Result<(), Error> {
    let Some(expected) = expected else {
        return Ok(());
    };

    if options.get("secret").map(String::as_str) == Some(expected) {
        return Ok(());
    }

    // 不记录服务端 secret，也不回显客户端提交的错误值。
    warn!("A WebSocket connection was rejected by legacy secret authentication.");
    Err(actix_web::error::ErrorUnauthorized("unauthorized"))
}

async fn handle_ws_connection(
    initial_id: Option<String>,
    principal: Option<ConnectionPrincipal>,
    is_transport: bool,
    mut session: actix_ws::Session,
    mut stream: impl StreamExt<Item = Result<AggregatedMessage, ProtocolError>> + Unpin,
    server: Addr<Server>,
    config: HttpConfig,
) {
    let (sender, receiver) = WsSender::channel(config.outbound_queue_capacity_value());
    let (mut outbound, mut overloaded) = receiver.into_parts();
    let (session_id, connection_token) = match server
        .send(Connect {
            id: initial_id,
            principal,
            is_transport,
            sender,
        })
        .await
    {
        Ok(result) => result,
        Err(error) => {
            warn!("[WS] Failed to register session: {:?}", error);
            let _ = session.close(None).await;
            return;
        }
    };

    let mut close_reason = None;
    loop {
        tokio::select! {
            Some(message) = outbound.recv() => {
                match tokio::time::timeout(
                    config.client_message_timeout_value(),
                    session.binary(message),
                ).await {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => break,
                    Err(_) => {
                        warn!("[WS] Outbound write timed out for {}", session_id);
                        close_reason = Some(CloseReason::from(CloseCode::Again));
                        break;
                    }
                }
            }
            changed = overloaded.changed() => {
                match changed {
                    Ok(()) if *overloaded.borrow() => {
                        warn!("[WS] Closing overloaded connection {}", session_id);
                        close_reason = Some(CloseReason::from(CloseCode::Again));
                        break;
                    }
                    Err(_) => break,
                    Ok(()) => {}
                }
            }
            message = stream.next() => {
                match message {
                    Some(Ok(AggregatedMessage::Binary(bytes))) => {
                        let message = match decode_message(&bytes) {
                            Ok(message) => message,
                            Err(_) => {
                                warn!("[WS] Closing connection after an invalid binary message");
                                close_reason = Some(CloseReason::from(CloseCode::Invalid));
                                break;
                            }
                        };

                        match tokio::time::timeout(
                            config.client_message_timeout_value(),
                            server.send(ClientMessage {
                                id: session_id.clone(),
                                data: message,
                            }),
                        )
                        .await
                        {
                            Ok(Ok(Some(error_message))) => {
                                let response = encode_message(
                                    &Message::new(&MessageType::Error)
                                        .text(&error_message)
                                        .build(),
                                );
                                let _ = tokio::time::timeout(
                                    config.client_message_timeout_value(),
                                    session.binary(response),
                                )
                                .await;
                                break;
                            }
                            Ok(Ok(None)) => {}
                            Ok(Err(error)) => {
                                warn!("[WS] Actor mailbox error: {:?}", error);
                                break;
                            }
                            Err(_) => {
                                warn!("[WS] Client message timed out");
                                close_reason = Some(CloseReason::from(CloseCode::Again));
                                break;
                            }
                        }
                    }
                    Some(Ok(AggregatedMessage::Close(_))) | None => break,
                    Some(Ok(AggregatedMessage::Ping(data))) => {
                        if tokio::time::timeout(
                            config.client_message_timeout_value(),
                            session.pong(&data),
                        )
                        .await
                        .is_err()
                        {
                            close_reason = Some(CloseReason::from(CloseCode::Again));
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(ProtocolError::Overflow)) => {
                        close_reason = Some(CloseReason::from(CloseCode::Size));
                        break;
                    }
                    Some(Err(error)) => {
                        warn!("[WS] Protocol error: {:?}", error);
                        break;
                    }
                }
            }
        }
    }

    server.do_send(Disconnect {
        id: session_id,
        token: connection_token,
    });
    let _ = tokio::time::timeout(
        config.client_message_timeout_value(),
        session.close(close_reason),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use actix::Actor;
    use actix_web::{http::StatusCode, test, web, App};
    use std::time::Duration;

    use crate::{ConnectionAuthError, ConnectionPrincipal, Server};

    use super::*;

    #[actix_web::test]
    async fn empty_legacy_client_id_keeps_random_id_behavior() {
        let mut options = HashMap::new();
        options.insert("client_id".to_owned(), String::new());

        assert_eq!(requested_legacy_id(&options), None);
        options.insert("client_id".to_owned(), "legacy-player".to_owned());
        assert_eq!(
            requested_legacy_id(&options),
            Some("legacy-player".to_owned())
        );
    }

    #[actix_web::test]
    async fn strict_origin_is_checked_before_authentication() {
        let auth_calls = Arc::new(AtomicUsize::new(0));
        let calls = auth_calls.clone();
        let http = HttpConfig::authenticated(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(ConnectionPrincipal::new("account", "session")) }
        })
        .allowed_origins(["https://game.example"]);
        let server = Server::new().debug(false).build().start();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(server))
                .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
                .route("/ws/", web::get().to(ws_route)),
        )
        .await;

        let request = test::TestRequest::get()
            .uri("/ws/?client_id=forged&secret=forged")
            .insert_header((header::ORIGIN, "https://evil.example"))
            .to_request();
        let response = test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(auth_calls.load(Ordering::SeqCst), 0);
    }

    #[actix_web::test]
    async fn strict_authentication_maps_failures_without_legacy_fallback() {
        let http = HttpConfig::authenticated(|_| async {
            Err(ConnectionAuthError::new("expired_session"))
        })
        .allowed_origins(["https://game.example"]);
        let server = Server::new().debug(false).build().start();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(server))
                .app_data(web::Data::new(HandshakeConfig {
                    secret: Some("legacy-secret".to_owned()),
                    http,
                }))
                .route("/ws/", web::get().to(ws_route)),
        )
        .await;

        let request = test::TestRequest::get()
            .uri("/ws/?client_id=forged&secret=legacy-secret")
            .insert_header((header::ORIGIN, "https://game.example"))
            .to_request();
        let response = test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(test::read_body(response).await.is_empty());
    }

    #[actix_web::test]
    async fn unavailable_authenticator_returns_service_unavailable() {
        let http = HttpConfig::authenticated(|_| async { Err(ConnectionAuthError::unavailable()) })
            .allowed_origins(["https://game.example"]);
        let server = Server::new().debug(false).build().start();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(server))
                .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
                .route("/ws/", web::get().to(ws_route)),
        )
        .await;

        let request = test::TestRequest::get()
            .uri("/ws/")
            .insert_header((header::ORIGIN, "https://game.example"))
            .to_request();
        let response = test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[actix_web::test]
    async fn authentication_timeout_fails_closed() {
        let http = HttpConfig::authenticated(|_| async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok(ConnectionPrincipal::new("account", "session"))
        })
        .allowed_origins(["https://game.example"])
        .auth_timeout(Duration::from_millis(1));
        let server = Server::new().debug(false).build().start();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(server))
                .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
                .route("/ws/", web::get().to(ws_route)),
        )
        .await;

        let request = test::TestRequest::get()
            .uri("/ws/")
            .insert_header((header::ORIGIN, "https://game.example"))
            .to_request();
        let response = test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[actix_web::test]
    async fn duplicate_origin_is_rejected_before_authentication() {
        let auth_calls = Arc::new(AtomicUsize::new(0));
        let calls = auth_calls.clone();
        let http = HttpConfig::authenticated(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok(ConnectionPrincipal::new("account", "session")) }
        })
        .allowed_origins(["https://game.example"]);
        let server = Server::new().debug(false).build().start();
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(server))
                .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
                .route("/ws/", web::get().to(ws_route)),
        )
        .await;

        let request = test::TestRequest::get()
            .uri("/ws/")
            .append_header((header::ORIGIN, "https://game.example"))
            .append_header((header::ORIGIN, "https://game.example"))
            .to_request();
        let response = test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(auth_calls.load(Ordering::SeqCst), 0);
    }
}
