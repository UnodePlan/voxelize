use std::fmt;

use tokio::sync::{mpsc, watch};

use crate::ConnectionPrincipal;

#[derive(Clone)]
pub(crate) struct PendingJoin {
    pub(crate) sender: WsSender,
    pub(crate) world_name: String,
    pub(crate) token: String,
    pub(crate) client_id: String,
    pub(crate) attempt_id: String,
    pub(crate) world_generation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetachedConnection {
    pub(crate) connection_id: String,
    pub(crate) world_name: String,
    pub(crate) client_id: String,
    pub(crate) attach_attempt_id: String,
    pub(crate) world_generation: String,
    pub(crate) connection_token: String,
}

#[derive(Clone)]
pub(crate) struct PendingRebind {
    pub(crate) detached: DetachedConnection,
    pub(crate) connection_id: String,
    pub(crate) connection_token: String,
    pub(crate) sender: WsSender,
    pub(crate) principal: ConnectionPrincipal,
    pub(crate) disconnected: bool,
    pub(crate) despawn_requested: bool,
}

#[derive(Clone)]
pub(crate) struct LeavingSession {
    pub(crate) sender: WsSender,
    pub(crate) token: String,
}

/// Bounded WebSocket output handle shared with World systems.
///
/// A full queue marks the connection as overloaded so the socket task closes
/// instead of silently dropping authoritative state.
#[derive(Clone)]
pub struct WsSender {
    sender: mpsc::Sender<Vec<u8>>,
    overloaded: watch::Sender<bool>,
    policy_close: watch::Sender<bool>,
}

impl WsSender {
    pub fn channel(capacity: usize) -> (Self, WsReceiver) {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        let (overloaded, overloaded_rx) = watch::channel(false);
        let (policy_close, policy_close_rx) = watch::channel(false);

        (
            Self {
                sender,
                overloaded,
                policy_close,
            },
            WsReceiver {
                receiver,
                overloaded: overloaded_rx,
                policy_close: policy_close_rx,
            },
        )
    }

    /// Queue a frame without blocking the synchronous World actor.
    pub fn send(&self, data: Vec<u8>) -> Result<(), WsSendError> {
        match self.sender.try_send(data) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                let _ = self.overloaded.send(true);
                Err(WsSendError::Overloaded)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(WsSendError::Closed),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    /// Request that the socket task closes this connection with a policy code.
    pub(crate) fn request_policy_close(&self) -> bool {
        if self.is_closed() || *self.policy_close.borrow() {
            return false;
        }

        self.policy_close.send(true).is_ok()
    }
}

pub struct WsReceiver {
    receiver: mpsc::Receiver<Vec<u8>>,
    overloaded: watch::Receiver<bool>,
    policy_close: watch::Receiver<bool>,
}

impl WsReceiver {
    pub async fn recv(&mut self) -> Option<Vec<u8>> {
        self.receiver.recv().await
    }

    pub async fn overloaded(&mut self) -> bool {
        if *self.overloaded.borrow() {
            return true;
        }

        self.overloaded.changed().await.is_ok() && *self.overloaded.borrow()
    }

    #[cfg(test)]
    pub(crate) fn policy_close_requested(&self) -> bool {
        *self.policy_close.borrow()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        mpsc::Receiver<Vec<u8>>,
        watch::Receiver<bool>,
        watch::Receiver<bool>,
    ) {
        (self.receiver, self.overloaded, self.policy_close)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WsSendError {
    Overloaded,
    Closed,
}

impl fmt::Display for WsSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Overloaded => "websocket output queue is full",
            Self::Closed => "websocket output queue is closed",
        })
    }
}

impl std::error::Error for WsSendError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn full_queue_marks_connection_overloaded() {
        let (sender, mut receiver) = WsSender::channel(1);
        sender.send(vec![1]).unwrap();

        assert_eq!(sender.send(vec![2]), Err(WsSendError::Overloaded));
        assert!(receiver.overloaded().await);
        assert_eq!(receiver.recv().await, Some(vec![1]));
    }

    #[tokio::test]
    async fn policy_close_request_is_delivered_once() {
        let (sender, receiver) = WsSender::channel(1);

        assert!(sender.request_policy_close());
        assert!(!sender.request_policy_close());
        assert!(receiver.policy_close_requested());
    }
}
