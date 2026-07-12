use actix_web::{
    cookie::{time::Duration as CookieDuration, Cookie, SameSite},
    http::{header::HeaderMap, header::COOKIE},
};

use super::AuthConfig;

pub fn session_cookie(config: &AuthConfig, token: String) -> Cookie<'static> {
    Cookie::build(config.cookie_name(), token)
        .http_only(true)
        .secure(config.cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(CookieDuration::seconds(config.session_ttl.whole_seconds()))
        .finish()
}

pub fn removal_cookie(config: &AuthConfig) -> Cookie<'static> {
    let mut cookie = Cookie::build(config.cookie_name(), "")
        .http_only(true)
        .secure(config.cookie_secure)
        .same_site(SameSite::Lax)
        .path("/")
        .finish();
    cookie.make_removal();
    cookie
}

pub(crate) fn cookie_value_from_headers(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(COOKIE)
        .filter_map(|header| header.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|value| Cookie::parse_encoded(value.trim()).ok())
        .find(|cookie| cookie.name() == name)
        .map(|cookie| cookie.value().to_owned())
}
