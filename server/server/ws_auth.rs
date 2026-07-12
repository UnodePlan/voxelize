use std::time::{Duration, SystemTime};

use tokio::time::Instant;

use crate::{ConnectionAuthRequest, ConnectionPrincipal, HttpConfig};

pub(super) struct AuthenticatedSessionGuard {
    config: HttpConfig,
    request: ConnectionAuthRequest,
    principal: ConnectionPrincipal,
    last_touch: Instant,
    last_touch_deadline: Option<SystemTime>,
    interval: Duration,
}

impl AuthenticatedSessionGuard {
    pub(super) fn new(
        config: &HttpConfig,
        request: ConnectionAuthRequest,
        principal: ConnectionPrincipal,
    ) -> Self {
        Self {
            config: config.clone(),
            request,
            principal,
            last_touch: Instant::now(),
            last_touch_deadline: None,
            interval: config.session_revalidation_interval_value(),
        }
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.principal.valid_until().map(|deadline| {
            let remaining = deadline
                .duration_since(SystemTime::now())
                .unwrap_or_default();
            Instant::now() + remaining
        })
    }

    pub(super) async fn revalidate(&mut self) -> bool {
        self.refresh(false).await
    }

    pub(super) async fn authorize_activity(&mut self) -> bool {
        let now = SystemTime::now();
        if self.principal.is_expired_at(now) {
            return false;
        }

        let deadline = self.principal.valid_until();
        let deadline_near = deadline.is_some_and(|deadline| {
            now.checked_add(self.interval)
                .is_none_or(|refresh_before| deadline <= refresh_before)
        });
        let near_deadline_changed = deadline_near && deadline != self.last_touch_deadline;
        if self.last_touch.elapsed() >= self.interval || near_deadline_changed {
            self.refresh(true).await
        } else {
            true
        }
    }

    async fn refresh(&mut self, touch: bool) -> bool {
        let request = self.request.clone();
        let verification = async {
            if touch {
                self.config.authenticate(request).await
            } else {
                self.config.revalidate(request).await
            }
        };
        let Ok(Ok(Some(actual))) =
            tokio::time::timeout(self.config.auth_timeout_value(), verification).await
        else {
            return false;
        };
        if !actual.is_valid()
            || !actual.same_identity(&self.principal)
            || actual.is_expired_at(SystemTime::now())
        {
            return false;
        }

        self.principal = actual;
        if touch {
            self.last_touch = Instant::now();
            self.last_touch_deadline = self.principal.valid_until();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use actix_web::http::header::HeaderMap;

    use crate::{ConnectionAuthFuture, ConnectionAuthenticator};

    use super::*;

    #[derive(Clone)]
    struct CountingAuthenticator {
        active_calls: Arc<AtomicUsize>,
        passive_calls: Arc<AtomicUsize>,
        result: ConnectionPrincipal,
    }

    impl ConnectionAuthenticator for CountingAuthenticator {
        fn authenticate(&self, _: ConnectionAuthRequest) -> ConnectionAuthFuture {
            self.active_calls.fetch_add(1, Ordering::SeqCst);
            let result = self.result.clone();
            Box::pin(async move { Ok(result) })
        }

        fn revalidate(&self, _: ConnectionAuthRequest) -> ConnectionAuthFuture {
            self.passive_calls.fetch_add(1, Ordering::SeqCst);
            let result = self.result.clone();
            Box::pin(async move { Ok(result) })
        }
    }

    #[actix_web::test]
    async fn passive_revalidation_does_not_refresh_idle_activity() {
        let (mut guard, active_calls, passive_calls) = guard_with_deadline(Duration::from_secs(60));

        assert!(guard.revalidate().await);
        assert_eq!(active_calls.load(Ordering::SeqCst), 0);
        assert_eq!(passive_calls.load(Ordering::SeqCst), 1);
    }

    #[actix_web::test]
    async fn activity_near_the_deadline_refreshes_before_forwarding() {
        let (mut guard, active_calls, passive_calls) = guard_with_deadline(Duration::from_secs(1));

        assert!(guard.authorize_activity().await);
        assert!(guard.authorize_activity().await);
        assert_eq!(active_calls.load(Ordering::SeqCst), 1);
        assert_eq!(passive_calls.load(Ordering::SeqCst), 0);
    }

    #[actix_web::test]
    async fn expired_deadline_is_rejected_before_any_authenticator_call() {
        let (mut guard, active_calls, passive_calls) = guard_with_deadline(Duration::ZERO);

        assert!(!guard.authorize_activity().await);
        assert_eq!(active_calls.load(Ordering::SeqCst), 0);
        assert_eq!(passive_calls.load(Ordering::SeqCst), 0);
    }

    fn guard_with_deadline(
        remaining: Duration,
    ) -> (
        AuthenticatedSessionGuard,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let active_calls = Arc::new(AtomicUsize::new(0));
        let passive_calls = Arc::new(AtomicUsize::new(0));
        let deadline = SystemTime::now() + remaining;
        let result = ConnectionPrincipal::new("account", "session").with_valid_until(deadline);
        let authenticator = CountingAuthenticator {
            active_calls: active_calls.clone(),
            passive_calls: passive_calls.clone(),
            result,
        };
        let config = HttpConfig::authenticated(authenticator)
            .allowed_origins(["https://game.example"])
            .session_revalidation_interval(Duration::from_secs(30));
        let principal = ConnectionPrincipal::new("account", "session").with_valid_until(deadline);
        let request = ConnectionAuthRequest {
            headers: HeaderMap::new(),
            peer_addr: None,
            path: "/ws/".to_owned(),
        };

        (
            AuthenticatedSessionGuard::new(&config, request, principal),
            active_calls,
            passive_calls,
        )
    }
}
