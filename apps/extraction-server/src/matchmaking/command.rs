use std::sync::{atomic::AtomicBool, Arc};

use tokio::sync::oneshot;
use uuid::Uuid;

use super::{MatchConnectionEvent, MatchmakingError, QueueSnapshot};
use crate::ports::MatchWorldRuntime;

pub(super) enum Command {
    BindRuntime {
        runtime: Arc<dyn MatchWorldRuntime>,
        reply: oneshot::Sender<()>,
    },
    Enqueue {
        account_id: Uuid,
        reply: oneshot::Sender<Result<QueueSnapshot, MatchmakingError>>,
    },
    Cancel {
        account_id: Uuid,
        reply: oneshot::Sender<Result<QueueSnapshot, MatchmakingError>>,
    },
    Connection {
        event: MatchConnectionEvent,
        reply: Option<oneshot::Sender<Result<(), MatchmakingError>>>,
    },
    Tick {
        reply: Option<oneshot::Sender<Result<(), MatchmakingError>>>,
        ticker_pending: Option<Arc<AtomicBool>>,
    },
    FailClosed,
}
