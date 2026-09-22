//! 파일 단위 증분 인덱싱 (계획 0037 Task 4).
//!
//! 무효화 단위가 파일인 이유는 DR-3에 있다 — 질의는 심볼 단위여야 쓸모 있지만, 무효화는
//! 파일 단위여야 싸다. 파일 해시 하나로 그 파일의 심볼 전체를 버리고 다시 넣는다.
//!
//! **커밋 해시가 아니라 파일 해시로 무효화한다.** 워크트리는 커밋되지 않은 변경이 정상
//! 상태이므로, 커밋 기준으로 판정하면 편집 중인 파일이 영원히 낡은 채로 남는다.

use sqlx::SqlitePool;

use crate::knowledge::hash::content_hash;
use crate::lspclient::protocol::RawSymbol;

/// `code_files` 한 행. 인덱싱 상태를 되짚을 때 쓴다.
#[derive(Debug, Clone, sqlx::FromRow, PartialEq, Eq)]
pub struct CodeFile {
    pub id: i64,
    pub worktree: String,
    pub rel_path: String,
    pub content_hash: String,
    pub lang: Option<String>,
    pub indexed_at: i64,
    /// LSP를 못 쓴 사유. `None`이면 정상 인덱싱됨.
    ///
    /// 빈 결과와 미지원을 구분하지 못하면 "데이터는 있는데 검색이 0건"이 되는데,
    /// 그것이 가장 진단하기 어려운 고장이다.
    pub skip_reason: Option<String>,
}

/// 이 파일을 다시 읽어야 하는가. 처음 보는 파일이면 참이다.
///
/// 해시가 같아도 `skip_reason`이 있으면 **다시 시도한다** — 지난번에 LSP 서버가 없어서
/// 건너뛴 파일은 서버를 설치하면 인덱싱돼야 한다. 내용이 안 바뀌었다는 이유로 영구히
/// 건너뛰면 설치가 반영되지 않는다.
pub async fn needs_reindex(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    hash: &str,
) -> anyhow::Result<bool> {
    let Some(file) = get_file(pool, worktree, rel_path).await? else {
        return Ok(true);
    };
    Ok(file.content_hash != hash || file.skip_reason.is_some())
}

pub async fn get_file(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
) -> anyhow::Result<Option<CodeFile>> {
    let row = sqlx::query_as::<_, CodeFile>(
        "SELECT id, worktree, rel_path, content_hash, lang, indexed_at, skip_reason \
         FROM code_files WHERE worktree = ? AND rel_path = ?",
    )
    .bind(worktree)
    .bind(rel_path)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// 파일 행을 넣거나 갱신하고 id를 준다. 재인덱싱이므로 `skip_reason`은 지운다.
pub async fn upsert_file(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    hash: &str,
    lang: Option<&str>,
    now: i64,
) -> anyhow::Result<i64> {
    sqlx::query(
        "INSERT INTO code_files (worktree, rel_path, content_hash, lang, indexed_at, skip_reason) \
         VALUES (?, ?, ?, ?, ?, NULL) \
         ON CONFLICT(worktree, rel_path) DO UPDATE SET \
           content_hash = excluded.content_hash, \
           lang = excluded.lang, \
           indexed_at = excluded.indexed_at, \
           skip_reason = NULL",
    )
    .bind(worktree)
    .bind(rel_path)
    .bind(hash)
    .bind(lang)
    .bind(now)
    .execute(pool)
    .await?;
    let (id,): (i64,) =
        sqlx::query_as("SELECT id FROM code_files WHERE worktree = ? AND rel_path = ?")
            .bind(worktree)
            .bind(rel_path)
            .fetch_one(pool)
            .await?;
    Ok(id)
}

/// 인덱싱하지 못한 파일을 사유와 함께 남긴다.
///
/// 조용히 건너뛰지 않는 것이 요점이다. 노드는 지운다 — 지난번에 인덱싱됐다가 이제 못 읽게
/// 됐다면, 남은 심볼은 확인할 수 없는 과거의 주장이다.
pub async fn record_skip(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO code_files (worktree, rel_path, content_hash, lang, indexed_at, skip_reason) \
         VALUES (?, ?, '', NULL, ?, ?) \
         ON CONFLICT(worktree, rel_path) DO UPDATE SET \
           content_hash = '', indexed_at = excluded.indexed_at, skip_reason = excluded.skip_reason",
    )
    .bind(worktree)
    .bind(rel_path)
    .bind(now)
    .bind(reason)
    .execute(pool)
    .await?;
    // 해시를 비워 두는 이유: 다음 실행에서 needs_reindex가 반드시 참이 되게 한다.
    sqlx::query("DELETE FROM code_nodes WHERE file_id = (SELECT id FROM code_files WHERE worktree = ? AND rel_path = ?)")
        .bind(worktree)
        .bind(rel_path)
        .execute(pool)
        .await?;
    Ok(())
}

/// 그 파일의 심볼을 통째로 갈아 끼운다. 다른 파일의 노드는 건드리지 않는다.
///
/// 엣지는 `ON DELETE CASCADE`로 함께 사라진다. 참조 엣지는 Task 5가 다시 세운다 —
/// 파일 하나가 바뀌면 그 파일을 **가리키던** 엣지도 낡으므로, 남겨 두는 편이 더 나쁘다.
pub async fn replace_nodes(
    pool: &SqlitePool,
    file_id: i64,
    symbols: &[RawSymbol],
) -> anyhow::Result<usize> {
    sqlx::query("DELETE FROM code_nodes WHERE file_id = ?")
        .bind(file_id)
        .execute(pool)
        .await?;
    let mut inserted = 0usize;
    for symbol in symbols {
        // 같은 이름이 같은 좌표에 두 번 오는 응답을 서버가 줄 수 있다(중첩 평탄화 결과).
        // UNIQUE 충돌로 인덱싱 전체를 실패시키지 않고 건너뛴다.
        let result = sqlx::query(
            "INSERT OR IGNORE INTO code_nodes \
             (file_id, name, kind, container, sel_line, sel_char, end_line) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(file_id)
        .bind(&symbol.name)
        .bind(symbol.kind)
        .bind(symbol.container.as_deref())
        .bind(symbol.sel_line)
        .bind(symbol.sel_char)
        .bind(symbol.end_line)
        .execute(pool)
        .await?;
        inserted += result.rows_affected() as usize;
    }
    Ok(inserted)
}

/// 파일 하나를 인덱싱한다. 내용이 그대로면 아무것도 하지 않고 `Unchanged`를 준다.
///
/// LSP 호출은 호출자가 넘긴 클로저가 한다 — 이 함수를 DB 계층에 두고 테스트하기 위해서다.
/// 클로저가 `Err`을 주면 그것이 곧 `skip_reason`이 된다(조용한 실패 금지).
pub async fn index_file<F, Fut>(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    text: &str,
    lang: Option<&str>,
    now: i64,
    fetch_symbols: F,
) -> anyhow::Result<Indexed>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<RawSymbol>, String>>,
{
    let hash = content_hash(text);
    if !needs_reindex(pool, worktree, rel_path, &hash).await? {
        return Ok(Indexed::Unchanged);
    }
    match fetch_symbols().await {
        Ok(symbols) => {
            let file_id = upsert_file(pool, worktree, rel_path, &hash, lang, now).await?;
            let count = replace_nodes(pool, file_id, &symbols).await?;
            Ok(Indexed::Symbols(count))
        }
        Err(reason) => {
            record_skip(pool, worktree, rel_path, &reason, now).await?;
            Ok(Indexed::Skipped(reason))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Indexed {
    /// 해시가 같아 건너뛰었다.
    Unchanged,
    /// 심볼 N개를 넣었다.
    Symbols(usize),
    /// 인덱싱하지 못했고 사유를 남겼다.
    Skipped(String),
}

// ── 참조 엣지 (Task 5) ──────────────────────────────────────────────────

/// 이 줄을 감싸는 **가장 안쪽** 심볼.
///
/// 어느 심볼에도 안 들어가면 `None`이다 — 파일 최상단의 `use` 같은 참조가 여기 해당한다.
/// 그럴 때 파일 노드로 떨어뜨리지 않는 것이 중요하다. 떨어뜨리면 "이 파일을 import한 모든
/// 곳"이 영향 범위에 들어와 답이 실질적으로 전부가 된다.
///
/// 가장 안쪽을 고르는 이유는 중첩 때문이다. `impl Foo { fn bar() {} }`에서 `bar` 안의
/// 참조는 `Foo`의 범위에도 들어가는데, 출발점은 `bar`여야 한다.
///
/// 범위의 시작으로 `sel_line`(이름 위치)을 쓴다 — 스키마에 본문 시작 줄이 없다. attribute나
/// doc comment 줄에 있는 참조는 이 근사에서 빠지는데, 그쪽은 호출이 아니라 대개 경로 언급이다.
pub fn enclosing(symbols: &[RawSymbol], line: u32) -> Option<&RawSymbol> {
    symbols
        .iter()
        .filter(|s| s.sel_line <= line && line <= s.end_line)
        .min_by_key(|s| s.end_line.saturating_sub(s.sel_line))
}

/// 엣지를 만들 값어치가 있는가.
///
/// 자기 자신으로 가는 엣지는 버린다. 정의 위치를 `references` 결과에 포함시키는 서버가
/// 있고(`includeDeclaration`을 꺼도 재귀 호출은 남는다), 자기 참조는 `impact_of`의 N홉
/// 순회에서 깊이만 태우고 답을 넓히지 않는다.
pub fn should_edge(src_id: i64, dst_id: i64) -> bool {
    src_id != dst_id
}

/// 그 파일의 그 줄을 감싸는 가장 안쪽 노드 id. 인덱싱되지 않은 파일이면 `None`.
///
/// [`enclosing`]의 DB판이다. 참조는 다른 파일에서 오므로 메모리의 심볼 목록만으로는 풀 수 없다.
pub async fn enclosing_node(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    line: u32,
) -> anyhow::Result<Option<i64>> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT n.id FROM code_nodes n JOIN code_files f ON n.file_id = f.id \
         WHERE f.worktree = ? AND f.rel_path = ? AND n.sel_line <= ? AND ? <= n.end_line \
         ORDER BY (n.end_line - n.sel_line) ASC, n.sel_line DESC LIMIT 1",
    )
    .bind(worktree)
    .bind(rel_path)
    .bind(line)
    .bind(line)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

/// `src → dst` 참조 엣지를 넣는다. 이미 있으면 조용히 넘어간다.
///
/// 방향은 **참조하는 쪽 → 참조되는 쪽**이다. `impact_of`는 이것을 거꾸로 타고 오른다.
pub async fn add_reference_edge(
    pool: &SqlitePool,
    src_id: i64,
    dst_id: i64,
) -> anyhow::Result<bool> {
    if !should_edge(src_id, dst_id) {
        return Ok(false);
    }
    let result =
        sqlx::query("INSERT OR IGNORE INTO code_edges (src_id, dst_id, rel) VALUES (?, ?, 'references')")
            .bind(src_id)
            .bind(dst_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}
