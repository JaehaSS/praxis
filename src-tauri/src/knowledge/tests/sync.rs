//! 링크 파싱·해소와 동기화 오케스트레이션 검증.

use super::test_pool;
use crate::knowledge::graph::Document;
use crate::knowledge::link::{parse_wikilinks, TargetIndex};
use crate::knowledge::sync::{load_cursor, sync_source, BoxChanges, Source, SourceChanges};

fn doc(external_id: &str, body: &str) -> Document {
    Document::embedded("obsidian", external_id, external_id.trim_end_matches(".md"), body)
}

struct FakeVault {
    docs: Vec<Document>,
    fail: bool,
}

impl Source for FakeVault {
    fn id(&self) -> &str {
        "obsidian"
    }
    fn changes<'a>(&'a self, _cursor: Option<&'a str>) -> BoxChanges<'a> {
        Box::pin(async move {
            if self.fail {
                anyhow::bail!("소스 조회 실패");
            }
            Ok(SourceChanges {
                upserts: self.docs.clone(),
                deletions: Vec::new(),
                next_cursor: Some("scan-1".into()),
                full_scan: true,
                has_more: false,
            })
        })
    }
}

// ── 링크 파싱 ──

#[test]
fn parses_aliases_anchors_and_embeds() {
    let links = parse_wikilinks("[[대상]] [[다른\u{7c}별칭]] [[문서#섹션]] ![[임베드]] [[]]");
    assert_eq!(links, vec!["대상", "다른", "문서", "임베드"]);
}

#[test]
fn ignores_unclosed_links_and_deduplicates() {
    // 닫히지 않은 `[[`는 링크가 아니다. 줄을 넘어 탐색하면 문서 전체를 삼킨다.
    let links = parse_wikilinks("[[열림\n다음줄]] [[중복]] [[중복]]");
    assert_eq!(links, vec!["중복"]);
}

// ── 대상 해소 ──

#[test]
fn resolves_by_stem_and_by_full_path() {
    let nodes = vec![(1, "폴더/노트.md".to_string()), (2, "다른.md".to_string())];
    let index = TargetIndex::build(&nodes);
    assert_eq!(index.resolve("노트"), Some(1));
    assert_eq!(index.resolve("폴더/노트"), Some(1));
    assert_eq!(index.resolve("다른.md"), Some(2));
    assert_eq!(index.resolve("없는노트"), None);
}

#[test]
fn ambiguous_stem_is_left_unresolved() {
    // 같은 이름이 두 폴더에 있으면 임의로 고르지 않는다 — 그래프가 조용히 틀려진다.
    let nodes = vec![(1, "a/같은.md".to_string()), (2, "b/같은.md".to_string())];
    assert_eq!(TargetIndex::build(&nodes).resolve("같은"), None);
}

// ── 동기화 ──

#[tokio::test]
async fn edges_resolve_regardless_of_scan_order() {
    // vault 순회 순서는 보장되지 않는다. 대상이 아직 없다고 엣지를 버리면
    // 알파벳 순서에 따라 그래프가 달라진다.
    let pool = test_pool().await;
    let vault = FakeVault {
        docs: vec![doc("a.md", "[[b]] 참조"), doc("b.md", "대상")],
        fail: false,
    };
    let report = sync_source(&pool, &vault, 10).await.unwrap();
    assert_eq!(report.indexed, 2);
    assert_eq!(report.edges, 1);

    let (edges,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_edges WHERE rel = 'links_to'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(edges, 1);
}

#[tokio::test]
async fn cursor_does_not_advance_when_the_source_fails() {
    // 커서를 먼저 올리면 중단된 구간이 영원히 스킵된다.
    let pool = test_pool().await;
    let vault = FakeVault {
        docs: Vec::new(),
        fail: true,
    };
    assert!(sync_source(&pool, &vault, 10).await.is_err());
    assert_eq!(load_cursor(&pool, "obsidian").await.unwrap(), None);
}

#[tokio::test]
async fn cursor_advances_after_a_successful_run() {
    let pool = test_pool().await;
    let vault = FakeVault {
        docs: vec![doc("a.md", "본문")],
        fail: false,
    };
    sync_source(&pool, &vault, 10).await.unwrap();
    assert_eq!(
        load_cursor(&pool, "obsidian").await.unwrap().as_deref(),
        Some("scan-1")
    );
}

#[tokio::test]
async fn resyncing_unchanged_vault_skips_everything_and_keeps_one_node_each() {
    let pool = test_pool().await;
    let vault = FakeVault {
        docs: vec![doc("a.md", "가"), doc("b.md", "나")],
        fail: false,
    };
    sync_source(&pool, &vault, 10).await.unwrap();
    let second = sync_source(&pool, &vault, 20).await.unwrap();
    assert_eq!(second.indexed, 0, "변경이 없는데 재색인했다");
    assert_eq!(second.skipped, 2);

    let (nodes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_nodes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(nodes, 2);
}

#[tokio::test]
async fn full_scan_detects_deletions_by_absence() {
    // 파일 삭제는 목록의 "부재"로만 드러난다. 명시적 diff가 없으면 영원히 검색된다.
    let pool = test_pool().await;
    let before = FakeVault {
        docs: vec![doc("keep.md", "유지"), doc("gone.md", "사라질 크세논")],
        fail: false,
    };
    sync_source(&pool, &before, 10).await.unwrap();

    let after = FakeVault {
        docs: vec![doc("keep.md", "유지")],
        fail: false,
    };
    let report = sync_source(&pool, &after, 20).await.unwrap();
    assert_eq!(report.deleted, 1);

    let (hits,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH '크세논'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hits, 0, "삭제한 노트가 아직 검색된다");
}

#[tokio::test]
async fn stale_edges_are_removed_when_a_link_disappears() {
    let pool = test_pool().await;
    let with_link = FakeVault {
        docs: vec![doc("a.md", "[[b]]"), doc("b.md", "대상")],
        fail: false,
    };
    sync_source(&pool, &with_link, 10).await.unwrap();

    let without = FakeVault {
        docs: vec![doc("a.md", "링크를 지웠다"), doc("b.md", "대상")],
        fail: false,
    };
    sync_source(&pool, &without, 20).await.unwrap();

    let (edges,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_edges")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(edges, 0, "사라진 링크의 엣지가 남았다");
}

// ── 리뷰에서 발견한 경계 ──

#[tokio::test]
async fn duplicate_vault_labels_are_rejected() {
    // 두 vault의 폴더 이름이 같으면 상대경로가 겹쳐 external_id가 충돌하고,
    // UNIQUE 제약 위에서 서로를 덮어쓴다 — 노트가 조용히 사라진다.
    use crate::knowledge::config::{MultiVault, VaultEntry};
    let entry = |root: &str| VaultEntry {
        root: root.into(),
        exclude: Vec::new(),
        embed_exclude: Vec::new(),
    };
    let source = MultiVault {
        entries: vec![entry("/tmp/a/노트"), entry("/tmp/b/노트")],
    };
    let err = source.changes(None).await.unwrap_err().to_string();
    assert!(err.contains("겹칩니다"), "충돌을 막지 않았다: {err}");
}

#[tokio::test]
async fn distinct_vault_labels_are_accepted() {
    use crate::knowledge::config::{MultiVault, VaultEntry};
    let entry = |root: &str| VaultEntry {
        root: root.into(),
        exclude: Vec::new(),
        embed_exclude: Vec::new(),
    };
    let source = MultiVault {
        entries: vec![entry("/tmp/a/하나"), entry("/tmp/b/둘")],
    };
    assert!(source.changes(None).await.is_ok());
}
