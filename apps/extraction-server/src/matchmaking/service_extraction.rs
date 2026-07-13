use super::{command::Command, MatchExtractionNotice, MatchmakingService};

impl MatchmakingService {
    pub(crate) fn observe_extraction(&self, notice: MatchExtractionNotice) -> bool {
        if !notice.is_valid() || self.gate.is_failed_closed() {
            self.fail_closed_after_overflow();
            return false;
        }
        if self.sender.try_send(Command::Extraction { notice }).is_ok() {
            true
        } else {
            self.fail_closed_after_overflow();
            false
        }
    }
}
