//! 코드 구조 그래프 — "이 심볼을 고치면 무엇이 깨지나"를 착수 **전에** 묻는다 (계획 0037).
//!
//! LSP가 이미 아는 것(심볼과 참조)을 워크트리 단위로 미리 긁어 질의 가능한 그래프로 세운다.
//! 기존 `lspclient`의 온디맨드 조회는 커서 위치가 있어야 답할 수 있어, 파일을 열기 전에는
//! 아무것도 물어볼 수 없었다.

pub mod build;
pub mod generation;
pub mod index;
pub mod jobs;
pub mod manifest;
pub mod neighborhood;
pub mod query;
mod reference_build;
pub mod schema;
pub mod snapshot;
pub mod status;
pub mod wiki;

use sqlx::SqlitePool;

/// 코드 그래프 스키마를 생성한다. 앱 기동마다 호출되므로 멱등해야 한다.
///
/// `CREATE TABLE IF NOT EXISTS`는 이미 있는 테이블을 그냥 건너뛴다 — 새 컬럼은 `MIGRATION`에
/// 적는 것만으로는 기존 DB에 영원히 생기지 않으므로 ALTER를 따로 넣는다(설계 0065 DR-3b).
/// `CREATE`와 `ALTER`를 같은 커넥션에서 끝내야 스키마 뷰가 도중에 갈라지지 않는다.
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut connection = pool.acquire().await?;
    sqlx::raw_sql(schema::MIGRATION)
        .execute(&mut *connection)
        .await?;
    // 기존 DB 보강 — 둘 다 nullable이라 이미 쌓인 행은 그대로 유효하다.
    for column in ["skip_reason TEXT", "edge_state TEXT"] {
        crate::db::add_column_if_missing(&mut *connection, "code_graph_files", column).await?;
    }
    Ok(())
}

/// 워크트리가 사라질 때 그 워크트리의 인덱싱을 통째로 지운다.
///
/// `code_files.worktree`가 가리키는 경로는 작업 종결과 함께 없어진다. 행을 남겨 두면 DB가
/// 단조 증가하고, 같은 경로가 재사용될 때 낡은 심볼이 새 코드인 척 섞인다.
/// 노드·엣지는 `ON DELETE CASCADE`가 따라 지운다 — 지우는 곳이 한 군데여야 빠뜨리지 않는다.
///
/// 지운 파일 행 수를 돌려준다. 인덱싱된 적 없는 워크트리면 0이고, 그것은 정상이다.
pub async fn purge_worktree(pool: &SqlitePool, worktree: &str) -> anyhow::Result<u64> {
    sqlx::query("DELETE FROM code_graph_runs WHERE worktree = ?")
        .bind(worktree)
        .execute(pool)
        .await?;
    let result = sqlx::query("DELETE FROM code_files WHERE worktree = ?")
        .bind(worktree)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod migrate_tests {
    use super::*;

    async fn columns(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info('code_graph_files')")
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_new_database_gets_the_reason_columns() {
        let pool = super::test_support::test_pool("migrate-new").await;

        let names = columns(&pool).await;

        assert!(names.contains(&"skip_reason".to_string()), "{names:?}");
        assert!(names.contains(&"edge_state".to_string()), "{names:?}");
    }

    #[tokio::test]
    async fn an_existing_database_gains_them_by_alter() {
        let pool = super::test_support::test_pool("migrate-old").await;
        // 옛 스키마 재현 — `CREATE TABLE IF NOT EXISTS`는 이 테이블을 건너뛴다.
        sqlx::query("DROP TABLE code_graph_files")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE code_graph_files (\
               id INTEGER PRIMARY KEY AUTOINCREMENT, run_id INTEGER NOT NULL, \
               rel_path TEXT NOT NULL, content_hash TEXT NOT NULL, lang TEXT NOT NULL, \
               UNIQUE(run_id, rel_path))",
        )
        .execute(&pool)
        .await
        .unwrap();

        migrate(&pool).await.unwrap();

        let names = columns(&pool).await;
        assert!(names.contains(&"skip_reason".to_string()), "{names:?}");
        assert!(names.contains(&"edge_state".to_string()), "{names:?}");
    }
}

#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod generation_query_tests;
#[cfg(test)]
mod neighborhood_tests;
#[cfg(test)]
mod generation_tests;
#[cfg(test)]
mod live;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
