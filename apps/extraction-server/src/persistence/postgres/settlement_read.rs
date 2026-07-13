use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    matchmaking::{SettlementRecord, SettlementResources},
    ports::SettlementRepositoryError,
};

pub(super) async fn find_settlement(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
) -> Result<Option<SettlementRecord>, SettlementRepositoryError> {
    let mut transaction = read_snapshot(pool).await?;
    let record = find_in_transaction(&mut transaction, match_id, account_id, false).await?;
    transaction
        .commit()
        .await
        .map_err(|_| SettlementRepositoryError::Unavailable)?;
    Ok(record)
}

pub(super) async fn find_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
    account_id: Uuid,
    lock: bool,
) -> Result<Option<SettlementRecord>, SettlementRepositoryError> {
    let suffix = if lock { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT id, match_id, account_id, idempotency_key, inventory_digest, \
         config_version, total_value, committed_at FROM extraction_settlements \
         WHERE match_id = $1 AND account_id = $2{suffix}"
    );
    let Some(row) = sqlx::query_as::<_, SettlementRow>(&query)
        .bind(match_id)
        .bind(account_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|_| SettlementRepositoryError::Unavailable)?
    else {
        return Ok(None);
    };
    let items = sqlx::query_as::<_, SettlementItemRow>(
        "SELECT item_key, quantity FROM settlement_items \
         WHERE settlement_id = $1 ORDER BY item_key",
    )
    .bind(row.id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| SettlementRepositoryError::Unavailable)?;
    let ledger_items = sqlx::query_as::<_, SettlementItemRow>(
        "SELECT item_key, delta AS quantity FROM asset_ledger \
         WHERE settlement_id = $1 ORDER BY item_key",
    )
    .bind(row.id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| SettlementRepositoryError::Unavailable)?;
    if items != ledger_items {
        return Err(SettlementRepositoryError::Invariant);
    }
    row.into_record(items).map(Some)
}

pub(super) async fn read_snapshot(
    pool: &PgPool,
) -> Result<Transaction<'_, Postgres>, SettlementRepositoryError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| SettlementRepositoryError::Unavailable)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(|_| SettlementRepositoryError::Unavailable)?;
    Ok(transaction)
}

#[derive(sqlx::FromRow)]
struct SettlementRow {
    id: Uuid,
    match_id: Uuid,
    account_id: Uuid,
    idempotency_key: String,
    inventory_digest: Vec<u8>,
    config_version: String,
    total_value: i64,
    committed_at: time::OffsetDateTime,
}

impl SettlementRow {
    fn into_record(
        self,
        items: Vec<SettlementItemRow>,
    ) -> Result<SettlementRecord, SettlementRepositoryError> {
        let mut resources = SettlementResources::default();
        for item in items {
            let quantity =
                u64::try_from(item.quantity).map_err(|_| SettlementRepositoryError::Invariant)?;
            match item.item_key.as_str() {
                "dirt" => resources.dirt = quantity,
                "gold" => resources.gold = quantity,
                "diamond" => resources.diamond = quantity,
                _ => return Err(SettlementRepositoryError::Invariant),
            }
        }
        let digest: [u8; 32] = self
            .inventory_digest
            .try_into()
            .map_err(|_| SettlementRepositoryError::Invariant)?;
        let record = SettlementRecord {
            settlement_id: self.id,
            match_id: self.match_id,
            account_id: self.account_id,
            idempotency_key: self.idempotency_key,
            inventory_digest: digest,
            config_version: self.config_version,
            resources,
            total_value: self.total_value,
            committed_at: self.committed_at,
        };
        validate_record(&record)?;
        Ok(record)
    }
}

#[derive(Debug, Eq, PartialEq, sqlx::FromRow)]
struct SettlementItemRow {
    item_key: String,
    quantity: i64,
}

pub(super) fn validate_record(record: &SettlementRecord) -> Result<(), SettlementRepositoryError> {
    let expected_key = format!("extract:v1:{}:{}", record.match_id, record.account_id);
    if record.settlement_id.is_nil()
        || record.match_id.is_nil()
        || record.account_id.is_nil()
        || record.idempotency_key != expected_key
        || record.inventory_digest != record.resources.digest()
        || record.resources.total_value(&record.config_version) != Some(record.total_value)
    {
        return Err(SettlementRepositoryError::Invariant);
    }
    Ok(())
}
