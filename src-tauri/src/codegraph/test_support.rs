use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::SqlitePool;

use crate::lspclient::protocol::RawSymbol;

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

pub async fn test_pool(label: &str) -> SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-codegraph-{label}-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    super::migrate(&pool).await.unwrap();
    pool
}

pub async fn count(pool: &SqlitePool, table: &str) -> i64 {
    let (count,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap();
    count
}

pub fn symbol(
    name: &str,
    selection: (u32, u32, u32, u32),
    body: (u32, u32, u32, u32),
) -> RawSymbol {
    RawSymbol {
        name: name.to_string(),
        kind: 12,
        container: None,
        sel_line: selection.0,
        sel_char: selection.1,
        sel_end_line: selection.2,
        sel_end_char: selection.3,
        body_start_line: body.0,
        body_start_char: body.1,
        body_end_line: body.2,
        body_end_char: body.3,
        end_line: body.2,
    }
}
