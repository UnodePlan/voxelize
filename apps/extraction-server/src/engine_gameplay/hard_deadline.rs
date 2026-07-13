use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use time::OffsetDateTime;

#[derive(Clone, Default)]
pub(crate) struct HardDeadlineControl(Arc<Mutex<HardDeadlineState>>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HardDeadlineRequest {
    pub monotonic_deadline: Duration,
    pub utc_deadline: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum HardDeadlineState {
    #[default]
    Open,
    Requested(HardDeadlineRequest),
    Terminalized(HardDeadlineRequest),
    Sealed(HardDeadlineRequest),
}

impl HardDeadlineControl {
    pub(crate) fn request(&self, request: HardDeadlineRequest) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        match *state {
            HardDeadlineState::Open => {
                *state = HardDeadlineState::Requested(request);
                true
            }
            HardDeadlineState::Requested(existing)
            | HardDeadlineState::Terminalized(existing)
            | HardDeadlineState::Sealed(existing) => existing == request,
        }
    }

    pub(crate) fn pending_request(&self) -> Option<HardDeadlineRequest> {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        match *state {
            HardDeadlineState::Requested(request) => Some(request),
            _ => None,
        }
    }

    pub(crate) fn mark_terminalized(&self, request: HardDeadlineRequest) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        match *state {
            HardDeadlineState::Requested(existing) if existing == request => {
                *state = HardDeadlineState::Terminalized(request);
                true
            }
            HardDeadlineState::Terminalized(existing) | HardDeadlineState::Sealed(existing) => {
                existing == request
            }
            _ => false,
        }
    }

    pub(crate) fn seal_after_outbox(&self) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        match *state {
            HardDeadlineState::Terminalized(request) => {
                *state = HardDeadlineState::Sealed(request);
                true
            }
            HardDeadlineState::Sealed(_) => true,
            _ => false,
        }
    }

    pub(crate) fn is_sealed(&self, request: HardDeadlineRequest) -> bool {
        matches!(
            *self.0.lock().unwrap_or_else(|error| error.into_inner()),
            HardDeadlineState::Sealed(existing) if existing == request
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(seconds: u64) -> HardDeadlineRequest {
        HardDeadlineRequest {
            monotonic_deadline: Duration::from_secs(seconds),
            utc_deadline: OffsetDateTime::UNIX_EPOCH
                + time::Duration::seconds(i64::try_from(seconds).unwrap()),
        }
    }

    #[test]
    fn seal_requires_matching_request_terminalization_and_flushed_outbox() {
        let control = HardDeadlineControl::default();
        let expected = request(720);

        assert!(!control.seal_after_outbox());
        assert!(control.request(expected));
        assert!(control.request(expected));
        assert!(!control.request(request(721)));
        assert!(!control.is_sealed(expected));
        assert!(control.mark_terminalized(expected));
        assert!(control.seal_after_outbox());
        assert!(control.seal_after_outbox());
        assert!(control.is_sealed(expected));
    }
}
