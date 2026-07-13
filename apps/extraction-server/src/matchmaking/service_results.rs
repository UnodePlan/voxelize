use uuid::Uuid;

use super::{command::Command, MatchResultRecord, MatchmakingError, MatchmakingService};

impl MatchmakingService {
    pub async fn find_match_result(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<MatchResultRecord>, MatchmakingError> {
        self.query(|reply| Command::FindMatchResult {
            match_id,
            account_id,
            reply,
        })
        .await
    }

    pub async fn find_latest_match_result(
        &self,
        account_id: Uuid,
    ) -> Result<Option<MatchResultRecord>, MatchmakingError> {
        self.query(|reply| Command::FindLatestMatchResult { account_id, reply })
            .await
    }
}
