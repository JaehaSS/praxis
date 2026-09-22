//! 메모리 스토어(FTS5) 통합 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::memory::{self, kind, tier};
use praxis_lib::projector;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_db() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir()
        .join(format!(
            "praxis-mem-test-{}-{}.sqlite",
            std::process::id(),
            n
        ))
        .to_string_lossy()
        .into_owned()
}

async fn setup() -> (sqlx::SqlitePool, String) {
    let path = temp_db();
    let pool = db::init_pool(&path).await.expect("init");
    memory::migrate(&pool).await.expect("migrate");
    (pool, path)
}

async fn insert_verified(
    pool: &sqlx::SqlitePool,
    tier_name: &str,
    scope_key: Option<&str>,
    legacy_kind: &str,
    content: &str,
    source_session: Option<&str>,
    now: i64,
) -> anyhow::Result<i64> {
    let knowledge_type = match legacy_kind {
        kind::DECISION => memory::knowledge_type::DECISION,
        kind::CONVENTION => memory::knowledge_type::CONVENTION,
        _ => memory::knowledge_type::CLAIM,
    };
    let id = memory::create_candidate(
        pool,
        tier_name,
        scope_key,
        knowledge_type,
        content,
        source_session,
        now,
    )
    .await?;
    memory::add_user_confirmation(pool, id, now, None).await?;
    memory::submit_for_review(pool, id, now).await?;
    memory::approve(pool, id, "human", now).await?;
    Ok(id)
}

/// 현재 벽시계 epoch초 — 휴면 필터(DORMANT_DAYS, 실시간 `strftime('%s','now')` 기준)와
/// 어긋나지 않도록, 휴면 판정에 영향받는 픽스처는 과거 고정값(1,2,3...) 대신 이 값을 쓴다.
fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn projection_task(pool: &sqlx::SqlitePool, worktree: &std::path::Path, now: i64) -> i64 {
    db::insert_task(
        pool,
        "/repo",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "project memory",
        Some("claude"),
        None,
        "terminal",
        now,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn insert_and_list_project() {
    let (pool, path) = setup().await;
    let id = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        kind::CONVENTION,
        "API 응답은 camelCase를 사용한다",
        Some("sess-1"),
        100,
    )
    .await
    .expect("insert");
    assert!(id > 0);
    let list = memory::list_project(&pool, "/repo").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, kind::CONVENTION);
    assert_eq!(list[0].confidence, 0.5);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn migration_backfills_legacy_memory_once_with_a_version_and_audit_event() {
    let path = temp_db();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
    let pool = db::init_pool(&path).await.expect("init");
    sqlx::raw_sql(
        "CREATE TABLE memories (\
           id INTEGER PRIMARY KEY AUTOINCREMENT, \
           tier TEXT NOT NULL, \
           scope_key TEXT, \
           kind TEXT NOT NULL, \
           content TEXT NOT NULL, \
           source_session TEXT, \
           confidence REAL NOT NULL DEFAULT 0.5, \
           usage_count INTEGER NOT NULL DEFAULT 0, \
           last_used INTEGER, \
           created_at INTEGER NOT NULL); \
         INSERT INTO memories (tier, scope_key, kind, content, created_at) \
           VALUES ('project', '/repo', 'fact', 'legacy claim', 100);",
    )
    .execute(&pool)
    .await
    .expect("legacy fixture");

    memory::migrate(&pool).await.expect("first migration");
    let (status, knowledge_type, version): (String, String, i64) = sqlx::query_as(
        "SELECT status, knowledge_type, current_version FROM memories WHERE content = 'legacy claim'",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy row is backfilled");
    assert_eq!(status, memory::knowledge_status::LEGACY_UNVERIFIED);
    assert_eq!(knowledge_type, memory::knowledge_type::CLAIM);
    assert_eq!(version, 1);

    let versions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    let events: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memory_events WHERE action = 'legacy_migrated'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(versions.0, 1);
    assert_eq!(events.0, 1);

    memory::migrate(&pool).await.expect("repeat migration");
    let versions_after: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_versions")
        .fetch_one(&pool)
        .await
        .unwrap();
    let events_after: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memory_events WHERE action = 'legacy_migrated'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        versions_after.0, 1,
        "repeat migration must not duplicate version"
    );
    assert_eq!(
        events_after.0, 1,
        "repeat migration must not duplicate audit event"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn create_candidate_stores_provenance_without_granting_injection_eligibility() {
    let (pool, path) = setup().await;
    let id = memory::create_candidate(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::OBSERVATION,
        "the parser accepts codex JSONL",
        Some("convo-task-42"),
        100,
    )
    .await
    .expect("candidate");

    let item = memory::list_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == id)
        .expect("candidate row");
    assert_eq!(item.status, memory::knowledge_status::CANDIDATE);
    assert_eq!(item.knowledge_type, memory::knowledge_type::OBSERVATION);
    assert_eq!(item.current_version, 1);
    assert_eq!(item.source_session.as_deref(), Some("convo-task-42"));
    assert!(!memory::is_injection_eligible(&item.status, false));

    let versions: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM memory_versions WHERE memory_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let events: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM memory_events WHERE memory_id = ? AND action = 'candidate_created'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(versions.0, 1);
    assert_eq!(events.0, 1);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn human_approval_requires_current_version_evidence() {
    let (pool, path) = setup().await;
    let id = memory::create_candidate(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::DECISION,
        "keep core memory logic Tauri-independent",
        Some("manual"),
        100,
    )
    .await
    .unwrap();

    memory::submit_for_review(&pool, id, 101).await.unwrap();
    assert!(memory::approve(&pool, id, "human", 102).await.is_err());
    assert!(memory::approve(&pool, id, "agent", 102).await.is_err());

    memory::add_user_confirmation(&pool, id, 103, None)
        .await
        .unwrap();
    memory::approve(&pool, id, "human", 104).await.unwrap();

    let item = memory::list_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.id == id)
        .unwrap();
    assert_eq!(item.status, memory::knowledge_status::VERIFIED);
    assert_eq!(item.verified_at, Some(104));
    let valid = memory::has_valid_evidence(&pool, id, item.current_version, 104)
        .await
        .unwrap();
    assert!(valid);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn fts_retrieve_ranks_relevant_and_scopes() {
    let (pool, path) = setup().await;
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        kind::DECISION,
        "authentication flow uses JWT tokens stored in httpOnly cookies",
        Some("s"),
        1,
    )
    .await
    .unwrap();
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        kind::FACT,
        "the database is PostgreSQL with sqlx",
        Some("s"),
        2,
    )
    .await
    .unwrap();
    // 다른 스코프 — 검색에 안 잡혀야 함
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/other"),
        kind::FACT,
        "authentication uses session cookies here too",
        Some("s"),
        3,
    )
    .await
    .unwrap();

    let hits = memory::retrieve_project(&pool, "/repo", "how does authentication work", 10)
        .await
        .unwrap();
    assert!(!hits.is_empty(), "should match authentication memory");
    assert!(
        hits[0].content.contains("JWT"),
        "JWT memory ranked first: {:?}",
        hits[0].content
    );
    assert!(
        hits.iter().all(|m| m.scope_key.as_deref() == Some("/repo")),
        "scope filtered"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn retrieve_layered_combines_project_global_and_excludes_other_scope() {
    let (pool, path) = setup().await;
    // usage_count=0이므로 created_at은 휴면 문턱(DORMANT_DAYS) 안쪽인 최근 시각을 써야 한다.
    let now = now_ts();
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        kind::DECISION,
        "auth uses JWT tokens",
        None,
        now,
    )
    .await
    .unwrap();
    insert_verified(
        &pool,
        tier::GLOBAL,
        None,
        kind::CONVENTION,
        "prefer JWT over sessions globally",
        None,
        now,
    )
    .await
    .unwrap();
    // 다른 레포 project — 제외되어야 함
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/other"),
        kind::FACT,
        "JWT here is irrelevant",
        None,
        now,
    )
    .await
    .unwrap();

    let hits = memory::retrieve_layered(&pool, "/repo", "JWT", 10)
        .await
        .unwrap();
    let contents: Vec<&str> = hits.iter().map(|m| m.content.as_str()).collect();
    assert!(
        contents.iter().any(|c| c.contains("auth uses JWT")),
        "project tier"
    );
    assert!(
        contents.iter().any(|c| c.contains("globally")),
        "global tier"
    );
    assert!(
        !contents.iter().any(|c| c.contains("irrelevant")),
        "other repo excluded"
    );
    assert_eq!(hits.len(), 2);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn inject_writes_and_replaces_marker_block_in_agents_md() {
    let (pool, path) = setup().await;
    // usage_count=0이므로 휴면 필터에 걸리지 않게 최근 created_at 사용.
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        kind::CONVENTION,
        "use camelCase for json",
        None,
        now_ts(),
    )
    .await
    .unwrap();

    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let wt = temp_root::dir().join(format!("praxis-inj-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join("AGENTS.md"), "# existing user content\n").unwrap();
    let task_id = projection_task(&pool, &wt, now_ts()).await;

    // 투영 대상은 AGENTS.md 하나다 — 벤더별 파일 분기는 ADR 2026-09-19로 물러났다.
    let targets = projector::project_targets();
    let injected = memory::inject_into_worktree(
        &pool,
        "/repo",
        "camelCase rule",
        None,
        task_id,
        now_ts(),
        &wt,
        8,
        &targets,
    )
    .await
    .unwrap();
    assert_eq!(injected, 1);
    let md = std::fs::read_to_string(wt.join("AGENTS.md")).unwrap();
    assert!(md.contains("PRAXIS MEMORY START"), "marker present");
    assert!(md.contains("use camelCase for json"), "memory present");
    assert!(
        md.contains("existing user content"),
        "user content preserved"
    );

    // 재주입: 마커 블록만 교체, 중복 없음
    memory::inject_into_worktree(
        &pool,
        "/repo",
        "camelCase rule",
        None,
        task_id,
        now_ts(),
        &wt,
        8,
        &targets,
    )
    .await
    .unwrap();
    let md2 = std::fs::read_to_string(wt.join("AGENTS.md")).unwrap();
    assert_eq!(
        md2.matches("PRAXIS MEMORY START").count(),
        1,
        "single block"
    );

    // 벤더별 파일은 더 이상 만들지 않는다 — 같은 블록이 두 곳에 갈라지면 한쪽만 고쳐진다.
    assert!(
        !wt.join("CLAUDE.md").exists(),
        "CLAUDE.md는 AGENTS.md로 대체됐다 — 더 이상 만들지 않는다"
    );
    assert!(
        !wt.join("GEMINI.md").exists(),
        "GEMINI.md는 AGENTS.md로 대체됐다 — 더 이상 만들지 않는다"
    );

    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn inject_removes_stale_managed_block_when_no_verified_knowledge_matches() {
    let (pool, path) = setup().await;
    let candidate = memory::create_candidate(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        "unverified text must not remain in context",
        Some("session"),
        now_ts(),
    )
    .await
    .unwrap();
    assert!(candidate > 0);

    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let wt = temp_root::dir().join(format!("praxis-clear-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(
        wt.join("AGENTS.md"),
        "# user content\n\n<!-- PRAXIS MEMORY START -->\n# Project Memory\n- [fact] old unverified text\n<!-- PRAXIS MEMORY END -->\n",
    )
    .unwrap();

    let targets = projector::project_targets();
    let task_id = projection_task(&pool, &wt, now_ts()).await;
    let injected = memory::inject_into_worktree(
        &pool,
        "/repo",
        "context",
        None,
        task_id,
        now_ts(),
        &wt,
        8,
        &targets,
    )
    .await
    .unwrap();
    assert_eq!(injected, 0);
    let text = std::fs::read_to_string(wt.join("AGENTS.md")).unwrap();
    assert!(text.contains("# user content"));
    assert!(!text.contains("PRAXIS MEMORY START"));
    assert!(!text.contains("old unverified text"));
    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn review_outcome_is_observational_and_never_filters_memory() {
    let (pool, path) = setup().await;
    let m = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "risky memory thing",
        None,
        1,
    )
    .await
    .unwrap();
    for t in 1..=3i64 {
        memory::record_injection(&pool, m, t, 10).await.unwrap();
        memory::record_review_outcome(&pool, t, memory::outcome::DISCARDED)
            .await
            .unwrap();
    }
    let got = memory::list_project(&pool, "/r").await.unwrap();
    assert_eq!(got[0].usage_count, 3);
    assert_eq!(
        got[0].confidence, 0.5,
        "관측 결과가 confidence를 바꾸면 안 됨"
    );
    let hits = memory::retrieve_layered(&pool, "/r", "risky", 10)
        .await
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "폐기된 작업의 주입 메모리도 자동 제외하지 않음"
    );
    let usages = memory::usages_for_memory(&pool, m).await.unwrap();
    assert!(usages
        .iter()
        .all(|usage| usage.outcome.as_deref() == Some("discarded")));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn dormant_unused_memory_excluded_but_used_memory_kept() {
    let (pool, path) = setup().await;
    let day: i64 = 24 * 3600;
    let now = now_ts();
    let old = now - 31 * day; // DORMANT_DAYS(30) 경과
    let dormant = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "dormant unused memory",
        None,
        old,
    )
    .await
    .unwrap();
    let used = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "dormant but used memory",
        None,
        old,
    )
    .await
    .unwrap();
    memory::record_injection(&pool, used, 1, old).await.unwrap(); // usage_count>=1 → 휴면 아님

    let hits = memory::retrieve_layered(&pool, "/r", "", 10).await.unwrap();
    let ids: Vec<i64> = hits.iter().map(|m| m.id).collect();
    assert!(
        !ids.contains(&dormant),
        "30일 경과·미사용 메모리는 후보에서 제외"
    );
    assert!(
        ids.contains(&used),
        "사용 이력 있으면 포함(가역 — 삭제 아님)"
    );

    // Rust측 판정(is_dormant)도 SQL 필터와 같은 기준.
    let all = memory::list_project(&pool, "/r").await.unwrap();
    let dormant_row = all.iter().find(|m| m.id == dormant).unwrap();
    let used_row = all.iter().find(|m| m.id == used).unwrap();
    assert!(memory::is_dormant(dormant_row, now), "미사용+경과 → 휴면");
    assert!(
        !memory::is_dormant(used_row, now),
        "사용 이력 있으면 휴면 아님"
    );

    // 메모리 자체는 삭제되지 않고 그대로 존재 — 후보 제외일 뿐.
    assert_eq!(
        all.len(),
        2,
        "휴면은 목록 조회에선 계속 보여야 함(삭제 아님)"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn cosine_similarity_basics() {
    assert!((memory::cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    assert!(memory::cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
    assert!((memory::cosine(&[1.0, 1.0], &[2.0, 2.0]) - 1.0).abs() < 1e-6); // 같은 방향
    assert_eq!(memory::cosine(&[1.0], &[1.0, 2.0]), 0.0); // 길이 불일치
}

#[test]
fn knowledge_contract_only_allows_verified_items_with_valid_evidence() {
    assert!(memory::knowledge_type::is_valid(
        memory::knowledge_type::CLAIM
    ));
    assert!(memory::knowledge_type::is_valid(
        memory::knowledge_type::OBSERVATION
    ));
    assert!(memory::knowledge_type::is_valid(
        memory::knowledge_type::DECISION
    ));
    assert!(memory::knowledge_type::is_valid(
        memory::knowledge_type::CONVENTION
    ));
    assert!(!memory::knowledge_type::is_valid("fact"));

    assert!(memory::is_injection_eligible(
        memory::knowledge_status::VERIFIED,
        true
    ));
    assert!(!memory::is_injection_eligible(
        memory::knowledge_status::VERIFIED,
        false
    ));
    assert!(!memory::is_injection_eligible(
        memory::knowledge_status::CANDIDATE,
        true
    ));
    assert!(!memory::is_injection_eligible(
        memory::knowledge_status::STALE,
        true
    ));
    assert!(!memory::is_injection_eligible(
        memory::knowledge_status::LEGACY_UNVERIFIED,
        true
    ));
}

#[test]
fn knowledge_transitions_preserve_human_review_gate() {
    assert!(memory::can_transition(
        memory::knowledge_status::CANDIDATE,
        memory::knowledge_status::PENDING_REVIEW,
    ));
    assert!(memory::can_transition(
        memory::knowledge_status::PENDING_REVIEW,
        memory::knowledge_status::VERIFIED,
    ));
    assert!(memory::can_transition(
        memory::knowledge_status::VERIFIED,
        memory::knowledge_status::STALE,
    ));
    assert!(!memory::can_transition(
        memory::knowledge_status::CANDIDATE,
        memory::knowledge_status::VERIFIED,
    ));
    assert!(!memory::can_transition(
        memory::knowledge_status::STALE,
        memory::knowledge_status::VERIFIED,
    ));
}

#[tokio::test]
async fn semantic_search_ranks_by_cosine_and_hybrid_merges() {
    let (pool, path) = setup().await;
    // usage_count=0이므로 휴면 필터에 걸리지 않게 최근 created_at 사용.
    let now = now_ts();
    // 손수 만든 임베딩(2차원, 모델 불필요)
    let a = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "near query",
        None,
        now,
    )
    .await
    .unwrap();
    let b = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "far from query",
        None,
        now,
    )
    .await
    .unwrap();
    let g = insert_verified(
        &pool,
        tier::GLOBAL,
        None,
        kind::FACT,
        "global near",
        None,
        now,
    )
    .await
    .unwrap();
    memory::set_embedding(&pool, a, &[1.0, 0.0]).await.unwrap(); // query와 동일 방향
    memory::set_embedding(&pool, b, &[0.0, 1.0]).await.unwrap(); // 직교
    memory::set_embedding(&pool, g, &[0.9, 0.1]).await.unwrap(); // query와 유사
                                                                 // 다른 스코프 — 제외
    let o = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/other"),
        kind::FACT,
        "other",
        None,
        now,
    )
    .await
    .unwrap();
    memory::set_embedding(&pool, o, &[1.0, 0.0]).await.unwrap();

    let q = [1.0f32, 0.0];
    let sem = memory::semantic_search(&pool, "/r", &q, 10).await.unwrap();
    assert_eq!(sem[0].id, a, "closest first");
    assert!(sem.iter().any(|m| m.id == g), "global included");
    assert!(!sem.iter().any(|m| m.id == o), "other scope excluded");
    assert!(
        sem.iter().position(|m| m.id == a).unwrap() < sem.iter().position(|m| m.id == b).unwrap()
    );

    // hybrid: 임베딩 없으면 FTS만, 있으면 융합 (여기선 FTS 텍스트 'near' + 시맨틱)
    let hy = memory::retrieve_hybrid(&pool, "/r", "near", Some(&q), 10)
        .await
        .unwrap();
    assert!(!hy.is_empty());
    let hy_fts_only = memory::retrieve_hybrid(&pool, "/r", "near", None, 10)
        .await
        .unwrap();
    assert!(hy_fts_only
        .iter()
        .all(|m| m.scope_key.as_deref() != Some("/other")));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn empty_query_returns_recent_and_purge_works() {
    let (pool, path) = setup().await;
    let a = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "alpha",
        None,
        1,
    )
    .await
    .unwrap();
    insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "beta",
        None,
        2,
    )
    .await
    .unwrap();
    let recent = memory::retrieve_project(&pool, "/r", "", 10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].content, "beta", "newest first");
    // 영구 삭제는 보관을 거쳐야 한다 — 되돌릴 수 없는 작업의 확인 단계다.
    memory::archive(&pool, a, 3).await.unwrap();
    memory::purge(&pool, a, 4).await.unwrap();
    assert_eq!(memory::list_project(&pool, "/r").await.unwrap().len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn extract_injected_block_pulls_marker_content() {
    let text = "user text before\n<!-- PRAXIS MEMORY START -->\n# Project Memory\n- [fact] use tabs\n<!-- PRAXIS MEMORY END -->\nuser text after";
    let block = memory::extract_injected_block(text).expect("block found");
    assert!(block.contains("use tabs"));
    assert!(!block.contains("user text"), "마커 밖 사용자 내용 제외");
    // 마커 없으면 None.
    assert!(memory::extract_injected_block("no markers here").is_none());
    // 엣지: START만 / END만 / 빈 블록 → None.
    assert!(memory::extract_injected_block("x <!-- PRAXIS MEMORY START --> no end").is_none());
    assert!(memory::extract_injected_block("<!-- PRAXIS MEMORY END --> after").is_none());
    assert!(
        memory::extract_injected_block("<!-- PRAXIS MEMORY START --><!-- PRAXIS MEMORY END -->")
            .is_none(),
        "빈 블록은 미주입으로 취급"
    );
    // 중복 START(부분쓰기 잔재) → END 앞 마지막 START만 추출.
    let dup = "<!-- PRAXIS MEMORY START -->\ngarbage\n<!-- PRAXIS MEMORY START -->\nreal\n<!-- PRAXIS MEMORY END -->";
    assert_eq!(memory::extract_injected_block(dup).as_deref(), Some("real"));
}

#[tokio::test]
async fn update_content_reindexes_fts_and_updates_kind() {
    let (pool, path) = setup().await;
    let id = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "old text alpha",
        None,
        1,
    )
    .await
    .unwrap();
    memory::update_content(&pool, id, "new text beta", kind::DECISION)
        .await
        .unwrap();
    let m = memory::list_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap();
    assert_eq!(m.content, "new text beta");
    assert_eq!(m.kind, kind::DECISION);
    // FTS 재색인(memories_au 트리거): 새 내용 잡히고 옛 내용은 안 잡힘.
    let new_hit = memory::retrieve_project(&pool, "/r", "beta", 10)
        .await
        .unwrap();
    assert!(new_hit.iter().any(|m| m.id == id), "새 내용 색인");
    let old_hit = memory::retrieve_project(&pool, "/r", "alpha", 10)
        .await
        .unwrap();
    assert!(!old_hit.iter().any(|m| m.id == id), "옛 내용 제거");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn update_content_nonexistent_is_noop() {
    let (pool, path) = setup().await;
    memory::update_content(&pool, 9999, "x", kind::FACT)
        .await
        .unwrap(); // 0행 → Ok
    assert!(
        memory::list_all(&pool).await.unwrap().is_empty(),
        "유령 행 생성 안 함"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn usages_for_memory_joins_task_and_reflects_outcome() {
    let (pool, path) = setup().await;
    let mid = insert_verified(&pool, tier::PROJECT, Some("/r"), kind::FACT, "m", None, 1)
        .await
        .unwrap();
    let tid = db::insert_task(
        &pool,
        "/r",
        "b",
        "main",
        "/p",
        "do the thing",
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    memory::record_injection(&pool, mid, tid, 2).await.unwrap();
    let u = memory::usages_for_memory(&pool, mid).await.unwrap();
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].task_id, tid);
    assert_eq!(u[0].instruction, "do the thing");
    assert!(u[0].outcome.is_none());
    memory::record_review_outcome(&pool, tid, memory::outcome::APPROVED)
        .await
        .unwrap();
    let u2 = memory::usages_for_memory(&pool, mid).await.unwrap();
    assert_eq!(u2[0].outcome.as_deref(), Some("approved"));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn injections_for_task_orders_and_isolates() {
    let (pool, path) = setup().await;
    let m1 = insert_verified(&pool, tier::PROJECT, Some("/r"), kind::FACT, "m1", None, 1)
        .await
        .unwrap();
    let m2 = insert_verified(&pool, tier::PROJECT, Some("/r"), kind::FACT, "m2", None, 1)
        .await
        .unwrap();
    memory::record_injection(&pool, m1, 42, 10).await.unwrap();
    memory::record_injection(&pool, m2, 42, 20).await.unwrap();
    memory::record_injection(&pool, m1, 99, 30).await.unwrap(); // 다른 작업 → 격리 확인용
    let inj = memory::injections_for_task(&pool, 42).await.unwrap();
    assert_eq!(inj.len(), 2, "작업 42의 주입만");
    assert!(inj[0].injected_at <= inj[1].injected_at, "주입 시각순");
    assert!(
        inj.iter().all(|i| i.exists),
        "존재하는 메모리는 exists=true"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn injection_report_tracks_and_survives_purge() {
    let (pool, path) = setup().await;
    let m = insert_verified(
        &pool,
        tier::PROJECT,
        Some("/r"),
        kind::FACT,
        "keep this",
        None,
        1,
    )
    .await
    .unwrap();
    memory::record_injection(&pool, m, 42, 100).await.unwrap();
    let inj = memory::injections_for_task(&pool, 42).await.unwrap();
    assert_eq!(inj.len(), 1);
    assert!(inj[0].exists);
    assert_eq!(inj[0].content.as_deref(), Some("keep this"));
    // 메모리를 영구 삭제해도 주입 기록은 남고 exists=false.
    memory::archive(&pool, m, 101).await.unwrap();
    memory::purge(&pool, m, 102).await.unwrap();
    let inj2 = memory::injections_for_task(&pool, 42).await.unwrap();
    assert_eq!(inj2.len(), 1);
    assert!(!inj2[0].exists);
    assert!(inj2[0].content.is_none());
    let _ = std::fs::remove_file(&path);
}
