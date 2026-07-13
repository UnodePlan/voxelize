use super::{command::Command, MatchDeathNotice, MatchTimeoutNotice, MatchmakingService};

impl MatchmakingService {
    /// World tick 不能等待数据库；通知进入有界协调器队列后再异步持久化。
    pub(crate) fn observe_death(&self, notice: MatchDeathNotice) -> bool {
        if !notice.is_valid() || self.gate.is_failed_closed() {
            self.fail_closed_after_overflow();
            return false;
        }
        if self.sender.try_send(Command::Death { notice }).is_ok() {
            true
        } else {
            self.fail_closed_after_overflow();
            false
        }
    }

    pub(crate) fn observe_timeout_elimination(&self, notice: MatchTimeoutNotice) -> bool {
        if !notice.is_valid() || self.gate.is_failed_closed() {
            self.fail_closed_after_overflow();
            return false;
        }
        if self
            .sender
            .try_send(Command::TimeoutElimination { notice })
            .is_ok()
        {
            true
        } else {
            self.fail_closed_after_overflow();
            false
        }
    }
}
