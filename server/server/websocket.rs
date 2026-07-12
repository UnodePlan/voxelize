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

use super::ws_auth::AuthenticatedSessionGuard;
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

    let (principal, reauth_request) = if config.http.requires_authentication() {
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
            config.http.authenticate(auth_request.clone()),
        )
        .await
        {
            Ok(Ok(Some(principal))) if principal.is_valid() => {
                (Some(principal), Some(auth_request))
            }
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
        (None, None)
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
        reauth_request,
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
    reauth_request: Option<ConnectionAuthRequest>,
    is_transport: bool,
    mut session: actix_ws::Session,
    mut stream: impl StreamExt<Item = Result<AggregatedMessage, ProtocolError>> + Unpin,
    server: Addr<Server>,
    config: HttpConfig,
) {
    let (sender, receiver) = WsSender::channel(config.outbound_queue_capacity_value());
    let (mut outbound, mut overloaded, mut policy_close) = receiver.into_parts();
    let mut auth_guard = match (principal.clone(), reauth_request) {
        (Some(principal), Some(request)) => {
            Some(AuthenticatedSessionGuard::new(&config, request, principal))
        }
        _ => None,
    };
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

    if let Some(guard) = auth_guard.as_mut() {
        if !guard.revalidate().await {
            // 首次握手认证和 Server 注册之间可能发生 logout；注册后再次校验，
            // 防止已撤销的 principal 进入可操作连接状态。
            server.do_send(Disconnect {
                id: session_id,
                token: connection_token,
            });
            let _ = session
                .close(Some(CloseReason::from(CloseCode::Policy)))
                .await;
            return;
        }
    }

    let mut revalidation_tick = tokio::time::interval(config.session_revalidation_interval_value());
    revalidation_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    revalidation_tick.tick().await;
    let mut close_reason = None;
    loop {
        let auth_deadline = auth_guard.as_ref().and_then(|guard| guard.deadline());
        tokio::select! {
            _ = wait_for_auth_deadline(auth_deadline) => {
                close_reason = Some(CloseReason::from(CloseCode::Policy));
                break;
            }
            _ = revalidation_tick.tick(), if auth_guard.is_some() => {
                let still_authenticated = match auth_guard.as_mut() {
                    Some(guard) => guard.revalidate().await,
                    None => true,
                };
                if !still_authenticated {
                    close_reason = Some(CloseReason::from(CloseCode::Policy));
                    break;
                }
            }
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
            changed = policy_close.changed() => {
                match changed {
                    Ok(()) if *policy_close.borrow() => {
                        // 会话撤销只通知 socket 关闭，Disconnect 仍负责既有连接状态清理。
                        close_reason = Some(CloseReason::from(CloseCode::Policy));
                        break;
                    }
                    Err(_) => break,
                    Ok(()) => {}
                }
            }
            message = stream.next() => {
                match message {
                    Some(Ok(AggregatedMessage::Binary(bytes))) => {
                        if let Some(guard) = auth_guard.as_mut() {
                            if !guard.authorize_activity().await {
                                close_reason = Some(CloseReason::from(CloseCode::Policy));
                                break;
                            }
                        }
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

async fn wait_for_auth_deadline(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
#[path = "websocket_tests.rs"]
mod tests;
