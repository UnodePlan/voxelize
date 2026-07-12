use sqlx::{query_scalar, Connection, PgConnection};

// 同一数据库当前只允许一个 matchmaking 进程执行启动恢复与承载内存 World。
const MATCHMAKING_PROCESS_LOCK_KEY: i64 = 0x564F_5845_4C50_5650;

pub struct MatchmakingProcessLock {
    _connection: PgConnection,
}

pub async fn acquire_matchmaking_process_lock(
    database_url: &str,
) -> Result<Option<MatchmakingProcessLock>, sqlx::Error> {
    let mut connection = PgConnection::connect(database_url).await?;
    let acquired = query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
        .bind(MATCHMAKING_PROCESS_LOCK_KEY)
        .fetch_one(&mut connection)
        .await?;
    Ok(acquired.then_some(MatchmakingProcessLock {
        _connection: connection,
    }))
}
