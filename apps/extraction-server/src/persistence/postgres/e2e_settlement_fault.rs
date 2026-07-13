use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::ports::SettlementRepositoryError;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum E2eSettlementFaultAction {
    ArmCommitUnknownAndBlockReads,
    ReleaseReads,
    Reset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct E2eSettlementFaultSnapshot {
    pub(crate) abort_calls: u64,
    pub(crate) commit_calls: u64,
    pub(crate) commit_outcome_unknowns: u64,
    pub(crate) mark_calls: u64,
    pub(crate) outcome_unknown_armed: bool,
    pub(crate) reads_blocked: bool,
    pub(crate) result_read_passthroughs: u64,
    pub(crate) result_read_unavailable: u64,
    pub(crate) settlement_read_passthroughs: u64,
    pub(crate) settlement_read_unavailable: u64,
}

#[derive(Debug, Default)]
pub(super) struct E2eSettlementFaultControl {
    abort_calls: AtomicU64,
    commit_calls: AtomicU64,
    commit_outcome_unknowns: AtomicU64,
    mark_calls: AtomicU64,
    outcome_unknown_armed: AtomicBool,
    reads_blocked: AtomicBool,
    result_read_passthroughs: AtomicU64,
    result_read_unavailable: AtomicU64,
    settlement_read_passthroughs: AtomicU64,
    settlement_read_unavailable: AtomicU64,
}

impl E2eSettlementFaultControl {
    pub(super) fn apply(&self, action: E2eSettlementFaultAction) -> E2eSettlementFaultSnapshot {
        match action {
            E2eSettlementFaultAction::ArmCommitUnknownAndBlockReads => {
                self.reset_counters();
                self.outcome_unknown_armed.store(true, Ordering::SeqCst);
                self.reads_blocked.store(true, Ordering::SeqCst);
            }
            E2eSettlementFaultAction::ReleaseReads => {
                self.reads_blocked.store(false, Ordering::SeqCst);
            }
            E2eSettlementFaultAction::Reset => {
                self.outcome_unknown_armed.store(false, Ordering::SeqCst);
                self.reads_blocked.store(false, Ordering::SeqCst);
                self.reset_counters();
            }
        }
        self.snapshot()
    }

    pub(super) fn record_mark(&self) {
        self.mark_calls.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn record_abort(&self) {
        self.abort_calls.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn record_commit(&self) {
        self.commit_calls.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn after_applied_commit(&self) -> Result<(), SettlementRepositoryError> {
        if !self.outcome_unknown_armed.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        self.commit_outcome_unknowns.fetch_add(1, Ordering::SeqCst);
        eprintln!(
            "{{\"event\":\"e2e_settlement_fault\",\"point\":\"after_commit_outcome_unknown\"}}"
        );
        Err(SettlementRepositoryError::OutcomeUnknown)
    }

    pub(super) fn before_settlement_read(&self) -> Result<(), SettlementRepositoryError> {
        self.before_read(
            &self.settlement_read_passthroughs,
            &self.settlement_read_unavailable,
        )
    }

    pub(super) fn before_result_read(&self) -> Result<(), SettlementRepositoryError> {
        self.before_read(
            &self.result_read_passthroughs,
            &self.result_read_unavailable,
        )
    }

    pub(super) fn snapshot(&self) -> E2eSettlementFaultSnapshot {
        E2eSettlementFaultSnapshot {
            abort_calls: self.abort_calls.load(Ordering::SeqCst),
            commit_calls: self.commit_calls.load(Ordering::SeqCst),
            commit_outcome_unknowns: self.commit_outcome_unknowns.load(Ordering::SeqCst),
            mark_calls: self.mark_calls.load(Ordering::SeqCst),
            outcome_unknown_armed: self.outcome_unknown_armed.load(Ordering::SeqCst),
            reads_blocked: self.reads_blocked.load(Ordering::SeqCst),
            result_read_passthroughs: self.result_read_passthroughs.load(Ordering::SeqCst),
            result_read_unavailable: self.result_read_unavailable.load(Ordering::SeqCst),
            settlement_read_passthroughs: self.settlement_read_passthroughs.load(Ordering::SeqCst),
            settlement_read_unavailable: self.settlement_read_unavailable.load(Ordering::SeqCst),
        }
    }

    fn before_read(
        &self,
        passthroughs: &AtomicU64,
        unavailable: &AtomicU64,
    ) -> Result<(), SettlementRepositoryError> {
        if self.reads_blocked.load(Ordering::SeqCst) {
            unavailable.fetch_add(1, Ordering::SeqCst);
            return Err(SettlementRepositoryError::Unavailable);
        }
        passthroughs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn reset_counters(&self) {
        for counter in [
            &self.abort_calls,
            &self.commit_calls,
            &self.commit_outcome_unknowns,
            &self.mark_calls,
            &self.result_read_passthroughs,
            &self.result_read_unavailable,
            &self.settlement_read_passthroughs,
            &self.settlement_read_unavailable,
        ] {
            counter.store(0, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_uncertainty_is_one_shot_and_reads_require_explicit_release() {
        let control = E2eSettlementFaultControl::default();
        control.apply(E2eSettlementFaultAction::ArmCommitUnknownAndBlockReads);
        control.record_mark();
        control.record_commit();

        assert_eq!(
            control.after_applied_commit(),
            Err(SettlementRepositoryError::OutcomeUnknown)
        );
        assert_eq!(control.after_applied_commit(), Ok(()));
        assert_eq!(
            control.before_settlement_read(),
            Err(SettlementRepositoryError::Unavailable)
        );
        assert_eq!(
            control.before_result_read(),
            Err(SettlementRepositoryError::Unavailable)
        );

        let blocked = control.snapshot();
        assert_eq!(blocked.mark_calls, 1);
        assert_eq!(blocked.commit_calls, 1);
        assert_eq!(blocked.commit_outcome_unknowns, 1);
        assert_eq!(blocked.settlement_read_unavailable, 1);
        assert_eq!(blocked.result_read_unavailable, 1);
        assert!(blocked.reads_blocked);
        assert!(!blocked.outcome_unknown_armed);

        control.apply(E2eSettlementFaultAction::ReleaseReads);
        assert_eq!(control.before_settlement_read(), Ok(()));
        assert_eq!(control.before_result_read(), Ok(()));
        let released = control.snapshot();
        assert_eq!(released.settlement_read_passthroughs, 1);
        assert_eq!(released.result_read_passthroughs, 1);
        assert!(!released.reads_blocked);
    }

    #[test]
    fn reset_disarms_faults_and_clears_evidence() {
        let control = E2eSettlementFaultControl::default();
        control.apply(E2eSettlementFaultAction::ArmCommitUnknownAndBlockReads);
        control.record_abort();
        control.apply(E2eSettlementFaultAction::Reset);

        assert_eq!(
            control.snapshot(),
            E2eSettlementFaultSnapshot {
                abort_calls: 0,
                commit_calls: 0,
                commit_outcome_unknowns: 0,
                mark_calls: 0,
                outcome_unknown_armed: false,
                reads_blocked: false,
                result_read_passthroughs: 0,
                result_read_unavailable: 0,
                settlement_read_passthroughs: 0,
                settlement_read_unavailable: 0,
            }
        );
    }
}
