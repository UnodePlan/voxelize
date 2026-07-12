use actix_web::{
    body::{EitherBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    http::header,
    middleware::Next,
    web, Error, HttpResponse,
};

use super::{ConnectionSecurityMode, HttpConfig};

pub(super) fn valid_exact_origin(origin: &str) -> bool {
    if origin.trim() != origin
        || origin.is_empty()
        || matches!(origin, "*" | "null")
        || origin.contains(['#', '?', '@'])
        || origin.ends_with('/')
    {
        return false;
    }

    let Ok(uri) = origin.parse::<actix_web::http::Uri>() else {
        return false;
    };
    let has_web_scheme = matches!(uri.scheme_str(), Some("http" | "https"));
    let has_authority = uri.authority().is_some();
    let has_no_path = uri
        .path_and_query()
        .map(|value| value.as_str() == "/")
        .unwrap_or(true);

    has_web_scheme && has_authority && has_no_path
}

fn request_origin_allowed(config: &HttpConfig, request: &ServiceRequest) -> bool {
    if config.security_mode() != ConnectionSecurityMode::PublicStrict {
        return true;
    }

    let mut origins = request.headers().get_all(header::ORIGIN).into_iter();
    let Some(origin) = origins.next() else {
        return false;
    };

    if origins.next().is_some() {
        return false;
    }

    let Ok(origin) = origin.to_str() else {
        return false;
    };

    valid_exact_origin(origin) && config.origin_allowed(Some(origin))
}

pub(crate) async fn strict_origin_guard<B>(
    config: web::Data<HttpConfig>,
    request: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, Error>
where
    B: MessageBody + 'static,
{
    // 严格模式在进入任何应用路由前统一收紧 Origin，避免各路由自行校验后产生遗漏。
    if !request_origin_allowed(config.get_ref(), &request) {
        return Ok(request
            .into_response(HttpResponse::Forbidden().finish())
            .map_into_right_body());
    }

    next.call(request)
        .await
        .map(|response| response.map_into_left_body())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_exact_web_origins() {
        assert!(valid_exact_origin("https://game.example"));
        assert!(valid_exact_origin("http://localhost:5173"));
        assert!(!valid_exact_origin("*"));
        assert!(!valid_exact_origin("null"));
        assert!(!valid_exact_origin("https://game.example/path"));
        assert!(!valid_exact_origin("https://game.example/"));
        assert!(!valid_exact_origin("https://game.example?query=1"));
        assert!(!valid_exact_origin("ftp://game.example"));
    }
}
