pub fn checked_out(pool: &sqlx::SqlitePool) -> u32 {
    pool.size().saturating_sub(pool.num_idle() as u32)
}

pub async fn wait_for_checked_out(pool: &sqlx::SqlitePool, minimum: u32) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if checked_out(pool) >= minimum {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("restore operations did not acquire their database connections");
}
