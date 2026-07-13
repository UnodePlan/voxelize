use sqlx::PgPool;
use time::OffsetDateTime;

use crate::{
    matchmaking::{CommitSettlement, ParticipantState, SettlementRecord},
    ports::{SettlementRepositoryError, TransitionOutcome},
};

use super::{
    matchmaking::read::{lock_match, lock_participant},
    settlement_assets::{
        classify_write_error, insert_assets, insert_settlement, preflight_warehouse,
    },
    settlement_read::{find_in_transaction, validate_record},
    settlement_write::{
        begin_write_transaction, map_match_error, unavailable, validate_qualification,
    },
};

pub(super) async fn commit(
    pool: &PgPool,
    command: CommitSettlement,
) -> Result<TransitionOutcome<SettlementRecord>, SettlementRepositoryError> {
    if !command.is_valid() {
        return Err(SettlementRepositoryError::Conflict);
    }
    let qualification = &command.qualification;
    let mut transaction = begin_write_transaction(pool).await?;
    // 所有结算统一按 match -> participant -> account -> settlement/assets 加锁，避免并发路径形成环形等待。
    let match_row = lock_match(&mut transaction, qualification.match_id)
        .await
        .map_err(map_match_error)?;
    let participant = lock_participant(
        &mut transaction,
        qualification.match_id,
        qualification.account_id,
    )
    .await
    .map_err(map_match_error)?;
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(qualification.account_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?
        .ok_or(SettlementRepositoryError::Conflict)?;
    if let Some(existing) = find_in_transaction(
        &mut transaction,
        qualification.match_id,
        qualification.account_id,
        true,
    )
    .await?
    {
        verify_existing(
            &command,
            &existing,
            participant.parsed_state().map_err(map_match_error)?,
            participant.settlement_qualified_at,
            participant.stats(),
        )?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(existing));
    }
    validate_qualification(&match_row, qualification)?;
    if participant.parsed_state().map_err(map_match_error)? != ParticipantState::SettlementPending
        || participant.settlement_qualified_at != Some(qualification.qualified_at)
        || participant.stats() != qualification.stats
    {
        return Err(SettlementRepositoryError::Conflict);
    }
    let database_now = sqlx::query_scalar::<_, OffsetDateTime>("SELECT transaction_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .map_err(unavailable)?;
    if match_row
        .settlement_grace_deadline
        .is_none_or(|deadline| database_now > deadline)
    {
        return Err(SettlementRepositoryError::WindowClosed);
    }

    let total_value = qualification
        .resources
        .total_value(&qualification.config_version)
        .ok_or(SettlementRepositoryError::Overflow)?;
    preflight_warehouse(
        &mut transaction,
        qualification.account_id,
        qualification.resources,
        total_value,
    )
    .await?;
    let record = SettlementRecord {
        settlement_id: command.settlement_id,
        match_id: qualification.match_id,
        account_id: qualification.account_id,
        idempotency_key: qualification.idempotency_key(),
        inventory_digest: qualification.inventory_digest,
        config_version: qualification.config_version.clone(),
        resources: qualification.resources,
        total_value,
        committed_at: database_now,
    };
    validate_record(&record)?;
    insert_settlement(&mut transaction, &record).await?;
    insert_assets(&mut transaction, &record).await?;
    let updated = sqlx::query(
        "UPDATE match_participants SET state = 'extracted', extracted_at = $3 \
         WHERE match_id = $1 AND account_id = $2 AND state = 'settlement_pending' \
           AND settlement_qualified_at = $4",
    )
    .bind(record.match_id)
    .bind(record.account_id)
    .bind(record.committed_at)
    .bind(qualification.qualified_at)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if updated.rows_affected() != 1 {
        return Err(SettlementRepositoryError::Conflict);
    }
    #[cfg(feature = "e2e-control")]
    super::e2e_settlement_crash::exit_if_configured(
        super::e2e_settlement_crash::SettlementCrashPoint::BeforeCommit,
    );
    // COMMIT 响应失败不能推断事务已回滚；调用方必须查询唯一结算记录后再收敛结果。
    transaction
        .commit()
        .await
        .map_err(|_| SettlementRepositoryError::OutcomeUnknown)?;
    #[cfg(feature = "e2e-control")]
    super::e2e_settlement_crash::exit_if_configured(
        super::e2e_settlement_crash::SettlementCrashPoint::AfterCommit,
    );
    Ok(TransitionOutcome::Applied(record))
}

fn verify_existing(
    command: &CommitSettlement,
    existing: &SettlementRecord,
    participant_state: ParticipantState,
    settlement_qualified_at: Option<OffsetDateTime>,
    participant_stats: crate::matchmaking::ParticipantMatchStats,
) -> Result<(), SettlementRepositoryError> {
    let qualification = &command.qualification;
    if participant_state != ParticipantState::Extracted
        || settlement_qualified_at != Some(qualification.qualified_at)
        || participant_stats != qualification.stats
        || existing.inventory_digest != qualification.inventory_digest
        || existing.config_version != qualification.config_version
        || existing.resources != qualification.resources
        || existing.idempotency_key != qualification.idempotency_key()
    {
        return Err(SettlementRepositoryError::Conflict);
    }
    Ok(())
}
