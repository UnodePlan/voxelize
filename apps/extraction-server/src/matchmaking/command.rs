use std::sync::{atomic::AtomicBool, Arc};

use tokio::sync::oneshot;
use uuid::Uuid;

use super::{MatchConnectionEvent, MatchResultRecord, MatchmakingError, QueueSnapshot};
#[cfg(any(feature = "engine", test))]
use super::{MatchDeathNotice, MatchExtractionNotice, MatchTimeoutNotice};
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
    FindQueueSnapshot {
        account_id: Uuid,
        reply: oneshot::Sender<Result<QueueSnapshot, MatchmakingError>>,
    },
    FindMatchResult {
        match_id: Uuid,
        account_id: Uuid,
        reply: oneshot::Sender<Result<Option<MatchResultRecord>, MatchmakingError>>,
    },
    FindLatestMatchResult {
        account_id: Uuid,
        reply: oneshot::Sender<Result<Option<MatchResultRecord>, MatchmakingError>>,
    },
    Connection {
        event: MatchConnectionEvent,
        reply: Option<oneshot::Sender<Result<(), MatchmakingError>>>,
    },
    #[cfg(any(feature = "engine", test))]
    Death {
        notice: MatchDeathNotice,
    },
    #[cfg(any(feature = "engine", test))]
    TimeoutElimination {
        notice: MatchTimeoutNotice,
    },
    #[cfg(any(feature = "engine", test))]
    Extraction {
        notice: MatchExtractionNotice,
    },
    HardDeadlineSealed {
        match_id: Uuid,
        world_name: String,
        world_generation: String,
        sealed: bool,
        world_stopped: bool,
    },
    Tick {
        reply: Option<oneshot::Sender<Result<(), MatchmakingError>>>,
        ticker_pending: Option<Arc<AtomicBool>>,
    },
    FailClosed,
}
