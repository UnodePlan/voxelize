mod role_probe;
mod rows;
#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    model::{
        OpsAccount, OpsLedgerEntry, OpsMatch, OpsPage, OpsPageRequest, OpsParticipant,
        OpsResourceCounts, OpsResourceQuantity, OpsSettlement, OpsWarehouse,
    },
    repository::{OpsRepository, OpsRepositoryError},
};
use role_probe::{RoleProbeRow, ROLE_PROBE_SQL};
use rows::{ParticipantRow, SettlementRow, WarehouseRow};

#[derive(Clone, Debug)]
pub(super) struct PgOpsRepository {
    pool: PgPool,
}

impl PgOpsRepository {
    pub(super) async fn connect(
        database_url: &str,
        statement_timeout: Duration,
    ) -> Result<Self, OpsRepositoryError> {
        let timeout = format!("{}ms", statement_timeout.as_millis());
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(2))
            .after_connect(move |connection, _| {
                let timeout = timeout.clone();
                Box::pin(async move {
                    // 只读事务只是纵深防护；启动角色探针才是权限门禁。
                    sqlx::query("SET default_transaction_read_only = on")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("SELECT set_config('search_path', 'pg_catalog,public', false)")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query(
                        "SELECT set_config('application_name', 'voxel-extraction-ops', false)",
                    )
                    .execute(&mut *connection)
                    .await?;
                    sqlx::query("SELECT set_config('statement_timeout', $1, false)")
                        .bind(timeout)
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("SELECT set_config('lock_timeout', '500ms', false)")
                        .execute(&mut *connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await
            .map_err(unavailable)?;
        let repository = Self { pool };
        repository.verify_read_only_role().await?;
        Ok(repository)
    }

    async fn verify_read_only_role(&self) -> Result<(), OpsRepositoryError> {
        let probe = sqlx::query_as::<_, RoleProbeRow>(ROLE_PROBE_SQL)
            .fetch_optional(&self.pool)
            .await
            .map_err(unavailable)?;
        probe
            .is_some_and(RoleProbeRow::is_safe)
            .then_some(())
            .ok_or(OpsRepositoryError::UnsafeRole)
    }

    async fn read_snapshot(&self) -> Result<Transaction<'_, Postgres>, OpsRepositoryError> {
        let mut transaction = self.pool.begin().await.map_err(unavailable)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(unavailable)?;
        Ok(transaction)
    }
}

#[async_trait]
impl OpsRepository for PgOpsRepository {
    async fn account(&self, account_id: Uuid) -> Result<Option<OpsAccount>, OpsRepositoryError> {
        let mut transaction = self.read_snapshot().await?;
        let value = sqlx::query_as::<_, OpsAccount>(
            "SELECT id, status, created_at, updated_at FROM accounts WHERE id = $1",
        )
        .bind(account_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(value)
    }

    async fn match_record(&self, match_id: Uuid) -> Result<Option<OpsMatch>, OpsRepositoryError> {
        let mut transaction = self.read_snapshot().await?;
        let value = sqlx::query_as::<_, OpsMatch>(
            "SELECT id, state, world_name, seed, generation_version, gameplay_version, \
                    config_version, created_at, started_at, extraction_open_at, hard_deadline, \
                    settlement_grace_deadline, finished_at, abort_reason \
             FROM matches WHERE id = $1",
        )
        .bind(match_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(value)
    }

    async fn participants(
        &self,
        match_id: Uuid,
        page: OpsPageRequest,
    ) -> Result<OpsPage<OpsParticipant>, OpsRepositoryError> {
        let mut transaction = self.read_snapshot().await?;
        let rows = sqlx::query_as::<_, ParticipantRow>(
            "SELECT match_id, account_id, public_player_id, seat_id, state, enqueued_at, \
                    reconnect_deadline, killed_by_account_id, extracted_at, \
                    settlement_qualified_at, terminal_cause, terminal_at, survived_ms, \
                    mined_counts, pickup_counts, lost_counts \
             FROM match_participants WHERE match_id = $1 \
             ORDER BY seat_id LIMIT $2 OFFSET $3",
        )
        .bind(match_id)
        .bind(i64::from(page.limit) + 1)
        .bind(i64::from(page.offset))
        .fetch_all(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        let items = rows
            .into_iter()
            .map(ParticipantRow::into_model)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(OpsPage::from_fetched(items, page))
    }

    async fn settlement(
        &self,
        settlement_id: Uuid,
    ) -> Result<Option<OpsSettlement>, OpsRepositoryError> {
        let mut transaction = self.read_snapshot().await?;
        let Some(row) = sqlx::query_as::<_, SettlementRow>(
            "SELECT id, match_id, account_id, config_version, total_value, committed_at \
             FROM extraction_settlements WHERE id = $1",
        )
        .bind(settlement_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?
        else {
            transaction.commit().await.map_err(unavailable)?;
            return Ok(None);
        };
        let items = sqlx::query_as::<_, OpsResourceQuantity>(
            "SELECT item_key, quantity FROM settlement_items \
             WHERE settlement_id = $1 ORDER BY item_key",
        )
        .bind(settlement_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        Ok(Some(row.into_model(items)?))
    }

    async fn warehouse(
        &self,
        account_id: Uuid,
    ) -> Result<Option<OpsWarehouse>, OpsRepositoryError> {
        let mut transaction = self.read_snapshot().await?;
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM accounts WHERE id = $1)")
                .bind(account_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(unavailable)?;
        if !exists {
            transaction.commit().await.map_err(unavailable)?;
            return Ok(None);
        }
        let rows = sqlx::query_as::<_, WarehouseRow>(
            "SELECT item_key, quantity, updated_at FROM warehouse_balances \
             WHERE account_id = $1 ORDER BY item_key",
        )
        .bind(account_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        let mut balances = OpsResourceCounts::default();
        let mut updated_at = None;
        for row in rows {
            if !balances.insert(&row.item_key, row.quantity) {
                return Err(OpsRepositoryError::Invariant);
            }
            updated_at = Some(updated_at.map_or(row.updated_at, |old: OffsetDateTime| {
                old.max(row.updated_at)
            }));
        }
        Ok(Some(OpsWarehouse {
            account_id,
            balances,
            updated_at,
        }))
    }

    async fn ledger(
        &self,
        account_id: Uuid,
        page: OpsPageRequest,
    ) -> Result<OpsPage<OpsLedgerEntry>, OpsRepositoryError> {
        let mut transaction = self.read_snapshot().await?;
        let items = sqlx::query_as::<_, OpsLedgerEntry>(
            "SELECT id, account_id, settlement_id, item_key, delta, created_at \
             FROM asset_ledger WHERE account_id = $1 \
             ORDER BY created_at DESC, id DESC LIMIT $2 OFFSET $3",
        )
        .bind(account_id)
        .bind(i64::from(page.limit) + 1)
        .bind(i64::from(page.offset))
        .fetch_all(&mut *transaction)
        .await
        .map_err(unavailable)?;
        transaction.commit().await.map_err(unavailable)?;
        if items
            .iter()
            .any(|item| item.delta <= 0 || !is_resource_key(&item.item_key))
        {
            return Err(OpsRepositoryError::Invariant);
        }
        Ok(OpsPage::from_fetched(items, page))
    }
}

fn unavailable(_: sqlx::Error) -> OpsRepositoryError {
    OpsRepositoryError::Unavailable
}

fn is_resource_key(key: &str) -> bool {
    matches!(key, "dirt" | "gold" | "diamond")
}
