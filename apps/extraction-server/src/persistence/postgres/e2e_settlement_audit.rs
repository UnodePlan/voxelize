use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use super::PgRepository;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct E2eSettlementAudit {
    match_state: String,
    participant_state: String,
    settlement_count: i64,
    settlement_item_count: i64,
    ledger_count: i64,
    warehouse_row_count: i64,
    settlement_resources: E2eResourceCounts,
    ledger_resources: E2eResourceCounts,
    warehouse_resources: E2eResourceCounts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct E2eResourceCounts {
    dirt: i64,
    gold: i64,
    diamond: i64,
}

#[derive(FromRow)]
struct AuditRow {
    match_state: String,
    participant_state: String,
    settlement_count: i64,
    settlement_item_count: i64,
    ledger_count: i64,
    warehouse_row_count: i64,
    settlement_dirt: i64,
    settlement_gold: i64,
    settlement_diamond: i64,
    ledger_dirt: i64,
    ledger_gold: i64,
    ledger_diamond: i64,
    warehouse_dirt: i64,
    warehouse_gold: i64,
    warehouse_diamond: i64,
}

pub(super) async fn load(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
) -> Result<Option<E2eSettlementAudit>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let row = sqlx::query_as::<_, AuditRow>(
        "SELECT m.state AS match_state, p.state AS participant_state, \
         (SELECT COUNT(*) FROM extraction_settlements s \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS settlement_count, \
         (SELECT COUNT(*) FROM settlement_items i JOIN extraction_settlements s \
            ON s.id = i.settlement_id WHERE s.match_id = m.id \
            AND s.account_id = p.account_id) AS settlement_item_count, \
         (SELECT COUNT(*) FROM asset_ledger l JOIN extraction_settlements s \
            ON s.id = l.settlement_id WHERE s.match_id = m.id \
            AND s.account_id = p.account_id) AS ledger_count, \
         (SELECT COUNT(*) FROM warehouse_balances w \
            WHERE w.account_id = p.account_id) AS warehouse_row_count, \
         (SELECT COALESCE(SUM(i.quantity) FILTER (WHERE i.item_key = 'dirt'), 0)::BIGINT \
            FROM settlement_items i JOIN extraction_settlements s ON s.id = i.settlement_id \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS settlement_dirt, \
         (SELECT COALESCE(SUM(i.quantity) FILTER (WHERE i.item_key = 'gold'), 0)::BIGINT \
            FROM settlement_items i JOIN extraction_settlements s ON s.id = i.settlement_id \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS settlement_gold, \
         (SELECT COALESCE(SUM(i.quantity) FILTER (WHERE i.item_key = 'diamond'), 0)::BIGINT \
            FROM settlement_items i JOIN extraction_settlements s ON s.id = i.settlement_id \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS settlement_diamond, \
         (SELECT COALESCE(SUM(l.delta) FILTER (WHERE l.item_key = 'dirt'), 0)::BIGINT \
            FROM asset_ledger l JOIN extraction_settlements s ON s.id = l.settlement_id \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS ledger_dirt, \
         (SELECT COALESCE(SUM(l.delta) FILTER (WHERE l.item_key = 'gold'), 0)::BIGINT \
            FROM asset_ledger l JOIN extraction_settlements s ON s.id = l.settlement_id \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS ledger_gold, \
         (SELECT COALESCE(SUM(l.delta) FILTER (WHERE l.item_key = 'diamond'), 0)::BIGINT \
            FROM asset_ledger l JOIN extraction_settlements s ON s.id = l.settlement_id \
            WHERE s.match_id = m.id AND s.account_id = p.account_id) AS ledger_diamond, \
         (SELECT COALESCE(SUM(w.quantity) FILTER (WHERE w.item_key = 'dirt'), 0)::BIGINT \
            FROM warehouse_balances w WHERE w.account_id = p.account_id) AS warehouse_dirt, \
         (SELECT COALESCE(SUM(w.quantity) FILTER (WHERE w.item_key = 'gold'), 0)::BIGINT \
            FROM warehouse_balances w WHERE w.account_id = p.account_id) AS warehouse_gold, \
         (SELECT COALESCE(SUM(w.quantity) FILTER (WHERE w.item_key = 'diamond'), 0)::BIGINT \
            FROM warehouse_balances w WHERE w.account_id = p.account_id) AS warehouse_diamond \
         FROM matches m JOIN match_participants p ON p.match_id = m.id \
         WHERE m.id = $1 AND p.account_id = $2",
    )
    .bind(match_id)
    .bind(account_id)
    .fetch_optional(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(row.map(Into::into))
}

impl PgRepository {
    pub(crate) async fn e2e_settlement_audit(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<E2eSettlementAudit>, sqlx::Error> {
        load(&self.pool, match_id, account_id).await
    }
}

impl From<AuditRow> for E2eSettlementAudit {
    fn from(row: AuditRow) -> Self {
        Self {
            match_state: row.match_state,
            participant_state: row.participant_state,
            settlement_count: row.settlement_count,
            settlement_item_count: row.settlement_item_count,
            ledger_count: row.ledger_count,
            warehouse_row_count: row.warehouse_row_count,
            settlement_resources: E2eResourceCounts {
                dirt: row.settlement_dirt,
                gold: row.settlement_gold,
                diamond: row.settlement_diamond,
            },
            ledger_resources: E2eResourceCounts {
                dirt: row.ledger_dirt,
                gold: row.ledger_gold,
                diamond: row.ledger_diamond,
            },
            warehouse_resources: E2eResourceCounts {
                dirt: row.warehouse_dirt,
                gold: row.warehouse_gold,
                diamond: row.warehouse_diamond,
            },
        }
    }
}
