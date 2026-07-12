use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchState {
    Waiting,
    Preparing,
    Active,
    ExtractionOpen,
    Settling,
    Finished,
    Aborted,
}

impl MatchState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Preparing => "preparing",
            Self::Active => "active",
            Self::ExtractionOpen => "extraction_open",
            Self::Settling => "settling",
            Self::Finished => "finished",
            Self::Aborted => "aborted",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Aborted)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Waiting, Self::Preparing | Self::Aborted)
                    | (Self::Preparing, Self::Active | Self::Aborted)
                    | (
                        Self::Active,
                        Self::ExtractionOpen | Self::Settling | Self::Aborted
                    )
                    | (Self::ExtractionOpen, Self::Settling | Self::Aborted)
                    | (Self::Settling, Self::Finished | Self::Aborted)
            )
    }

    pub fn transition_to(self, next: Self) -> Result<Self, TransitionError> {
        self.can_transition_to(next)
            .then_some(next)
            .ok_or(TransitionError::Match {
                from: self,
                to: next,
            })
    }
}

impl FromStr for MatchState {
    type Err = StateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "waiting" => Ok(Self::Waiting),
            "preparing" => Ok(Self::Preparing),
            "active" => Ok(Self::Active),
            "extraction_open" => Ok(Self::ExtractionOpen),
            "settling" => Ok(Self::Settling),
            "finished" => Ok(Self::Finished),
            "aborted" => Ok(Self::Aborted),
            _ => Err(StateParseError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantState {
    Waiting,
    Preparing,
    Active,
    Disconnected,
    SettlementPending,
    Dead,
    Extracted,
    TimedOut,
    Aborted,
}

impl ParticipantState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Preparing => "preparing",
            Self::Active => "active",
            Self::Disconnected => "disconnected",
            Self::SettlementPending => "settlement_pending",
            Self::Dead => "dead",
            Self::Extracted => "extracted",
            Self::TimedOut => "timed_out",
            Self::Aborted => "aborted",
        }
    }

    pub const fn occupies_nonterminal_seat(self) -> bool {
        matches!(
            self,
            Self::Waiting
                | Self::Preparing
                | Self::Active
                | Self::Disconnected
                | Self::SettlementPending
        )
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Dead | Self::Extracted | Self::TimedOut | Self::Aborted
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Waiting, Self::Preparing | Self::Aborted)
                    | (Self::Preparing, Self::Active | Self::Aborted)
                    | (
                        Self::Active,
                        Self::Disconnected
                            | Self::SettlementPending
                            | Self::Dead
                            | Self::TimedOut
                            | Self::Aborted
                    )
                    | (
                        Self::Disconnected,
                        Self::Active | Self::Dead | Self::TimedOut | Self::Aborted
                    )
                    | (Self::SettlementPending, Self::Extracted | Self::Aborted)
            )
    }

    pub fn transition_to(self, next: Self) -> Result<Self, TransitionError> {
        self.can_transition_to(next)
            .then_some(next)
            .ok_or(TransitionError::Participant {
                from: self,
                to: next,
            })
    }
}

impl FromStr for ParticipantState {
    type Err = StateParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "waiting" => Ok(Self::Waiting),
            "preparing" => Ok(Self::Preparing),
            "active" => Ok(Self::Active),
            "disconnected" => Ok(Self::Disconnected),
            "settlement_pending" => Ok(Self::SettlementPending),
            "dead" => Ok(Self::Dead),
            "extracted" => Ok(Self::Extracted),
            "timed_out" => Ok(Self::TimedOut),
            "aborted" => Ok(Self::Aborted),
            _ => Err(StateParseError),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateParseError;

impl fmt::Display for StateParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unknown match state")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionError {
    Match {
        from: MatchState,
        to: MatchState,
    },
    Participant {
        from: ParticipantState,
        to: ParticipantState,
    },
}
