use sqlx::PgPool;
use uuid::Uuid;

use crate::ports::{AuthRepositoryError, WarehouseSnapshot, WarehouseStats};

pub(super) async fn load_warehouse(
    pool: &PgPool,
    account_id: Uuid,
) -> Result<WarehouseSnapshot, AuthRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    let balances = sqlx::query_as::<_, BalanceRow>(
        "SELECT item_key, quantity FROM warehouse_balances WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(unavailable)?;

    let mut snapshot = WarehouseSnapshot::default();
    for balance in balances {
        match balance.item_key.as_str() {
            "dirt" => snapshot.dirt = balance.quantity,
            "gold" => snapshot.gold = balance.quantity,
            "diamond" => snapshot.diamond = balance.quantity,
            _ => return Err(AuthRepositoryError::Unavailable),
        }
    }

    let total_resources_extracted = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(ledger.delta), 0)::BIGINT \
         FROM asset_ledger AS ledger WHERE ledger.account_id = $1",
    )
    .bind(account_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let stats = sqlx::query_as::<_, StatsRow>(
        "SELECT \
             COALESCE(SUM(total_value), 0)::BIGINT AS total_extraction_value, \
             COUNT(id)::BIGINT AS successful_extractions, \
             COALESCE(MAX(total_value), 0)::BIGINT AS highest_single_match_value \
         FROM extraction_settlements AS settlements \
         WHERE settlements.account_id = $1",
    )
    .bind(account_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    snapshot.stats = stats.with_total_resources(total_resources_extracted);
    transaction.commit().await.map_err(unavailable)?;
    Ok(snapshot)
}

#[derive(sqlx::FromRow)]
struct BalanceRow {
    item_key: String,
    quantity: i64,
}

#[derive(sqlx::FromRow)]
struct StatsRow {
    total_extraction_value: i64,
    successful_extractions: i64,
    highest_single_match_value: i64,
}

impl StatsRow {
    fn with_total_resources(self, total_resources_extracted: i64) -> WarehouseStats {
        WarehouseStats {
            total_resources_extracted,
            total_extraction_value: self.total_extraction_value,
            successful_extractions: self.successful_extractions,
            highest_single_match_value: self.highest_single_match_value,
        }
    }
}

fn unavailable(_: sqlx::Error) -> AuthRepositoryError {
    AuthRepositoryError::Unavailable
}
