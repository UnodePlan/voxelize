mod create;
mod lifecycle;
mod participants;
pub(in crate::persistence::postgres) mod read;
mod recovery;
pub(in crate::persistence::postgres) mod rows;
mod timeouts;

use sqlx::Error;

use crate::ports::MatchRepositoryError;

pub(super) use create::{abort, activate, create_preparing};
pub(super) use lifecycle::{begin_settling, finish, open_extraction};
pub(super) use participants::{mark_dead, mark_disconnected, reconnect};
pub(super) use read::{find_match, find_nonterminal_by_account};
pub(super) use recovery::abort_unrecoverable_matches;
pub(super) use timeouts::mark_timed_out;

fn classify_write_error(error: Error) -> MatchRepositoryError {
    let Error::Database(database_error) = &error else {
        return MatchRepositoryError::Unavailable;
    };
    match database_error.code().as_deref() {
        Some("23505")
            if database_error.constraint() == Some("match_participants_active_account_idx") =>
        {
            MatchRepositoryError::SeatOccupied
        }
        Some("23502" | "23503" | "23505" | "23514") => MatchRepositoryError::Conflict,
        _ => MatchRepositoryError::Unavailable,
    }
}

fn unavailable(_: Error) -> MatchRepositoryError {
    MatchRepositoryError::Unavailable
}
