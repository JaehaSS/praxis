//! `knowledge` 모듈 테스트 공용 헬퍼.

mod chunk;
mod gmail;
mod gmail_auth;
mod gmail_normalize;
mod gmail_resume;
mod golden;
mod isolation;
mod obsidian;
mod schema;
mod search;
mod sync;
mod upsert;

use std::sync::atomic::{AtomicU32, Ordering};

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 임시 파일 DB — `sqlite::memory:`는 풀의 커넥션마다 별개의 빈 DB를 보므로 쓰지 않는다.
/// (마이그레이션한 커넥션과 조회하는 커넥션이 달라 "테이블이 없다"로 실패한다.)
/// `insights/outcomes/tests/database.rs:80`과 같은 관례.
pub async fn test_pool() -> sqlx::SqlitePool {
    let pool = raw_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    pool
}

/// `migrate`를 부르지 않은 빈 DB. 구버전 스키마를 손으로 세운 뒤 업그레이드 경로를
/// 검증할 때 쓴다 — `test_pool`은 이미 최신 스키마라 그 경로를 재현할 수 없다.
pub async fn raw_pool() -> sqlx::SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-knowledge-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    crate::db::init_pool(path.to_str().unwrap()).await.unwrap()
}
