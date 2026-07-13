use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    matchmaking::{SettlementRecord, SettlementResources},
    ports::SettlementRepositoryError,
};

pub(super) async fn preflight_warehouse(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
    resources: SettlementResources,
    total_value: i64,
) -> Result<(), SettlementRepositoryError> {
    let dirt = i64::try_from(resources.dirt).map_err(|_| SettlementRepositoryError::Overflow)?;
    let gold = i64::try_from(resources.gold).map_err(|_| SettlementRepositoryError::Overflow)?;
    let diamond =
        i64::try_from(resources.diamond).map_err(|_| SettlementRepositoryError::Overflow)?;
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT item_key, quantity FROM warehouse_balances WHERE account_id = $1 \
         ORDER BY item_key FOR UPDATE",
    )
    .bind(account_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(unavailable)?;
    for (key, current) in rows {
        let added = resource_items(resources)
            .into_iter()
            .find_map(|(item_key, quantity)| (item_key == key).then_some(quantity))
            .ok_or(SettlementRepositoryError::Invariant)?;
        current
            .checked_add(i64::try_from(added).map_err(|_| SettlementRepositoryError::Overflow)?)
            .ok_or(SettlementRepositoryError::Overflow)?;
    }
    let (resources_fit, value_fits) = sqlx::query_as::<_, (bool, bool)>(
        "SELECT \
           (SELECT COALESCE(SUM(delta::NUMERIC), 0) FROM asset_ledger WHERE account_id = $1) \
             + $2::BIGINT::NUMERIC + $3::BIGINT::NUMERIC + $4::BIGINT::NUMERIC \
             <= $6::BIGINT::NUMERIC, \
           (SELECT COALESCE(SUM(total_value::NUMERIC), 0) \
              FROM extraction_settlements WHERE account_id = $1) \
             + $5::BIGINT::NUMERIC <= $6::BIGINT::NUMERIC",
    )
    .bind(account_id)
    .bind(dirt)
    .bind(gold)
    .bind(diamond)
    .bind(total_value)
    .bind(i64::MAX)
    .fetch_one(&mut **transaction)
    .await
    .map_err(unavailable)?;
    if !resources_fit || !value_fits {
        return Err(SettlementRepositoryError::Overflow);
    }
    Ok(())
}

pub(super) async fn insert_settlement(
    transaction: &mut Transaction<'_, Postgres>,
    record: &SettlementRecord,
) -> Result<(), SettlementRepositoryError> {
    sqlx::query(
        "INSERT INTO extraction_settlements (id, match_id, account_id, idempotency_key, \
         inventory_digest, config_version, total_value, committed_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(record.settlement_id)
    .bind(record.match_id)
    .bind(record.account_id)
    .bind(&record.idempotency_key)
    .bind(record.inventory_digest.as_slice())
    .bind(&record.config_version)
    .bind(record.total_value)
    .bind(record.committed_at)
    .execute(&mut **transaction)
    .await
    .map_err(classify_write_error)?;
    Ok(())
}

pub(super) async fn insert_assets(
    transaction: &mut Transaction<'_, Postgres>,
    record: &SettlementRecord,
) -> Result<(), SettlementRepositoryError> {
    for (item_key, quantity) in resource_items(record.resources) {
        if quantity == 0 {
            continue;
        }
        let quantity = i64::try_from(quantity).map_err(|_| SettlementRepositoryError::Overflow)?;
        sqlx::query(
            "INSERT INTO settlement_items (settlement_id, item_key, quantity) VALUES ($1,$2,$3)",
        )
        .bind(record.settlement_id)
        .bind(item_key)
        .bind(quantity)
        .execute(&mut **transaction)
        .await
        .map_err(classify_write_error)?;
        sqlx::query(
            "INSERT INTO asset_ledger \
             (id, account_id, settlement_id, item_key, delta, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(Uuid::new_v4())
        .bind(record.account_id)
        .bind(record.settlement_id)
        .bind(item_key)
        .bind(quantity)
        .bind(record.committed_at)
        .execute(&mut **transaction)
        .await
        .map_err(classify_write_error)?;
        sqlx::query(
            "INSERT INTO warehouse_balances (account_id,item_key,quantity,updated_at) \
             VALUES ($1,$2,$3,$4) ON CONFLICT (account_id,item_key) DO UPDATE SET \
             quantity = warehouse_balances.quantity + EXCLUDED.quantity, updated_at = EXCLUDED.updated_at",
        )
        .bind(record.account_id)
        .bind(item_key)
        .bind(quantity)
        .bind(record.committed_at)
        .execute(&mut **transaction)
        .await
        .map_err(classify_write_error)?;
    }
    Ok(())
}

fn resource_items(resources: SettlementResources) -> [(&'static str, u64); 3] {
    [
        ("diamond", resources.diamond),
        ("dirt", resources.dirt),
        ("gold", resources.gold),
    ]
}

pub(super) fn classify_write_error(error: sqlx::Error) -> SettlementRepositoryError {
    match &error {
        sqlx::Error::Database(database) if database.code().as_deref() == Some("22003") => {
            SettlementRepositoryError::Overflow
        }
        sqlx::Error::Database(database)
            if matches!(
                database.code().as_deref(),
                Some("23502" | "23503" | "23505" | "23514")
            ) =>
        {
            SettlementRepositoryError::Conflict
        }
        _ => SettlementRepositoryError::Unavailable,
    }
}

fn unavailable(_: sqlx::Error) -> SettlementRepositoryError {
    SettlementRepositoryError::Unavailable
}
