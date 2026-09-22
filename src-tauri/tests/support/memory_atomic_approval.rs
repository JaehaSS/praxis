use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::SqlitePool;

static NEXT_DATABASE: AtomicU32 = AtomicU32::new(1);

pub struct ApprovalFixture {
    pub pool: SqlitePool,
    pub path: String,
    pub memory_id: i64,
}

impl ApprovalFixture {
    pub async fn candidate(label: &str) -> Self {
        let path = database_path(label);
        remove_database_files(&path);
        let pool = praxis_lib::db::init_pool(&path).await.unwrap();
        praxis_lib::memory::migrate(&pool).await.unwrap();
        let memory_id = praxis_lib::memory::create_candidate(
            &pool,
            praxis_lib::memory::tier::PROJECT,
            Some("/repo"),
            praxis_lib::memory::knowledge_type::CONVENTION,
            &format!("atomic approval {label}"),
            Some("test"),
            100,
        )
        .await
        .unwrap();
        Self {
            pool,
            path,
            memory_id,
        }
    }

    pub async fn set_status(&self, status: &str) {
        sqlx::query("UPDATE memories SET status = ? WHERE id = ?")
            .bind(status)
            .bind(self.memory_id)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    pub async fn cleanup(self) {
        self.pool.close().await;
        remove_database_files(&self.path);
    }
}

pub async fn count_where(pool: &SqlitePool, sql: &str, memory_id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(memory_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn database_path(label: &str) -> String {
    let serial = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
    super::temp_root::dir()
        .join(format!(
            "praxis-atomic-approval-{}-{label}-{serial}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

fn remove_database_files(path: &str) {
    for target in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        let _ = std::fs::remove_file(target);
    }
}
