//! 학습 퀴즈·인사이트 덱 커맨드.
//!
//! `commands.rs`에서 갈라져 나왔다 — 모든 기능이 한 파일 끝에 줄을 붙이는
//! 구조가 병행 worktree의 머지 충돌을 만들었다.


use sqlx::SqlitePool;
use tauri::{Manager, State};

use crate::db::{self};

use super::{AppState, now, pool_of};

/// 다음 문제. 먼저 inbox를 한 번 거둔다 — 생성 작업이 끝난 직후 첫 출제부터 반영된다.
#[tauri::command]
pub async fn quiz_next(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kinds: Vec<String>,
) -> Result<Option<crate::quiz::serve::QuizItem>, String> {
    let pool = pool_of(&state)?;
    let dir = quiz_inbox_dir(&app);
    // 수집 실패가 출제를 막지 않는다 — 이미 쌓인 문제로 계속 낼 수 있다.
    if let Err(e) = crate::quiz::inbox::collect_and_store(&pool, &dir, now()).await {
        eprintln!("퀴즈 inbox 수집 실패: {e}");
    }
    crate::quiz::serve::next(&pool, &kinds, now())
        .await
        .map_err(|e| e.to_string())
}

/// 큐에 무엇이 남았는지만 본다 — 패널을 띄울지 정하는 폴링용이다.
///
/// `quiz_next`와 달리 시도를 열지 않는다. 대신 **inbox 수집은 여기서도 한다** — 생성 작업이
/// 만든 문제가 DB로 들어오는 통로가 그것뿐이라, 프로브가 건너뛰면 "낼 문제 0 → 패널 안 뜸 →
/// `quiz_next` 호출 안 됨 → 영영 0"인 교착이 된다.
#[tauri::command]
pub async fn quiz_availability(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kinds: Vec<String>,
) -> Result<crate::quiz::serve::Availability, String> {
    let pool = pool_of(&state)?;
    let dir = quiz_inbox_dir(&app);
    if let Err(e) = crate::quiz::inbox::collect_and_store(&pool, &dir, now()).await {
        eprintln!("퀴즈 inbox 수집 실패: {e}");
    }
    crate::quiz::serve::availability(&pool, &kinds)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn quiz_answer(
    state: State<'_, AppState>,
    item_id: i64,
    picked: String,
) -> Result<Option<crate::quiz::serve::AnswerResult>, String> {
    let pool = pool_of(&state)?;
    crate::quiz::serve::answer(&pool, item_id, &picked, now())
        .await
        .map_err(|e| e.to_string())
}

/// 신고 — 이 문제는 다시 나오지 않는다.
#[tauri::command]
pub async fn quiz_report(state: State<'_, AppState>, item_id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    crate::quiz::serve::report(&pool, item_id, now())
        .await
        .map_err(|e| e.to_string())
}

/// 검수 통과 — 이 시점부터 출제 대상이 된다.
#[tauri::command]
pub async fn quiz_approve(state: State<'_, AppState>, item_id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    crate::quiz::serve::approve(&pool, item_id, now())
        .await
        .map_err(|e| e.to_string())
}

/// 검수 대기 목록 (도메인 문제).
#[tauri::command]
pub async fn quiz_pending(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<crate::quiz::serve::PendingItem>, String> {
    let pool = pool_of(&state)?;
    crate::quiz::serve::pending(&pool, limit.unwrap_or(50))
        .await
        .map_err(|e| e.to_string())
}

/// 인사이트 덱 디렉터리 — 퀴즈 inbox와 같은 자리(앱 데이터 밑).
fn insight_decks_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(crate::insightdeck::DECKS_SUBDIR)
}

/// 최근에 보인 카드 키 — 새것이 앞이다.
///
/// **새 테이블을 만들지 않는다.** 열 개짜리 목록에 마이그레이션과 스키마를 붙이는 것은
/// 과하다. `settings` 한 행이면 충분하고, 형식이 바뀌어도 잃을 것이 없다(재출제 억제는
/// 틀려도 카드가 한 번 겹칠 뿐이다).
const INSIGHT_RECENT_KEY: &str = "insight_recent";

async fn insight_recent(pool: &SqlitePool) -> Vec<String> {
    db::get_setting(pool, INSIGHT_RECENT_KEY)
        .await
        .ok()
        .flatten()
        .map(|v| v.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

#[derive(serde::Serialize)]
pub struct InsightAvailability {
    /// 실제로 띄울 수 있는 카드 수 — 덱 파일 카드 + 지식창고 카드. **토글과 무관한 사실이다.**
    pub cards: u32,
    /// 덱 파일 수.
    pub decks: u32,
    /// 덱 파일에서 나온 카드 수.
    pub deck_cards: u32,
    /// 개인 지식창고에서 읽은 문서 수(카드가 안 나온 문서 포함). 창고가 연결돼 있지 않으면 0.
    pub wiki_notes: u32,
    /// 지식창고 문서에서 잘라 낸 카드 수.
    pub wiki_cards: u32,
    /// 카드를 읽은 지식창고 루트. `None`이면 연결된 창고가 없다 — 설정 화면이 그 사실을 말한다.
    pub wiki_root: Option<String>,
    /// 창고의 최상위 폴더 목록(범위 밖 폴더 포함) — 설정 화면이 범위를 고르는 재료다.
    pub wiki_folders: Vec<crate::insightdeck::wiki::VaultFolder>,
    /// 카드로 쓸 폴더 범위. `None`이면 전체(아직 안 고른 상태), 빈 목록이면 지식창고 카드 없음.
    pub wiki_scope: Option<Vec<String>>,
    /// 기능이 켜져 있는가. **게이트는 `enabled && cards > 0`으로 판단한다**(ADR 0114).
    ///
    /// 토글을 여기서 카드 0으로 접지 않는 이유는 설정 화면 때문이다 — 껐다고 "덱 0개"로
    /// 보이면 사용자는 덱을 잃은 줄 안다. **가용성은 현실을 보고하고 판단은 게이트가 한다.**
    pub enabled: bool,
    /// 로드 중 버린 것들 — 설정 화면이 보여 준다. 조용히 버리면 왜 안 뜨는지 알 수 없다.
    pub warnings: Vec<String>,
}

/// 카드의 두 원천을 한 번에 읽은 결과. 가용성과 다음 카드가 같은 것을 본다.
struct InsightSources {
    decks: Vec<crate::insightdeck::deck::Deck>,
    deck_files: u32,
    deck_cards: u32,
    wiki_notes: u32,
    wiki_cards: u32,
    wiki_root: Option<String>,
    wiki_folders: Vec<crate::insightdeck::wiki::VaultFolder>,
    wiki_scope: Option<Vec<String>>,
    warnings: Vec<String>,
}

/// 카드로 쓸 지식창고 폴더 범위 — `settings` 한 행, JSON 배열. 행이 없거나 `null`이면 전체다.
///
/// **새 테이블을 만들지 않는다**(`insight_recent`와 같은 이유). 폴더 이름은 `/`·공백·따옴표를
/// 품을 수 있어 줄 단위 대신 JSON으로 둔다.
const INSIGHT_WIKI_FOLDERS_KEY: &str = "insight_wiki_folders";

async fn insight_wiki_scope(pool: &SqlitePool) -> Option<Vec<String>> {
    let raw = db::get_setting(pool, INSIGHT_WIKI_FOLDERS_KEY)
        .await
        .ok()
        .flatten()?;
    serde_json::from_str::<Option<Vec<String>>>(&raw).ok().flatten()
}

/// 덱 파일과 지식창고 문서를 모두 읽는다. 둘 다 디스크라 blocking 풀로 분리한다.
///
/// 지식창고는 **연결된(활성) 창고만** 읽는다. 연결을 끊은 창고를 계속 읽으면 "끊었는데 왜
/// 내 글이 뜨나"가 된다. 루트 정체가 바뀐 창고는 경고로 남기고 카드 0장으로 본다.
async fn insight_sources(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
) -> Result<InsightSources, String> {
    let dir = insight_decks_dir(app);
    let mut warnings = Vec::new();
    let vault_root = match crate::knowledge::vault::active_vault_root(pool).await {
        Ok(root) => root,
        Err(e) => {
            warnings.push(format!("지식창고를 확인하지 못했습니다: {e}"));
            None
        }
    };
    let wiki_root = vault_root
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    let wiki_scope = insight_wiki_scope(pool).await;
    let scope_for_load = wiki_scope.clone();
    let (decks, deck_warnings, wiki) = tauri::async_runtime::spawn_blocking(move || {
        let (decks, warnings) = crate::insightdeck::deck::load_decks(&dir);
        let wiki = vault_root
            .as_deref()
            .map(|root| crate::insightdeck::wiki::load_vault_cards(root, scope_for_load.as_deref()))
            .unwrap_or_default();
        (decks, warnings, wiki)
    })
    .await
    .map_err(|e| e.to_string())?;
    warnings.extend(deck_warnings);
    warnings.extend(wiki.warnings);
    let deck_files = decks.len() as u32;
    let deck_cards = decks.iter().map(|d| d.cards.len() as u32).sum();
    let wiki_cards = wiki.decks.iter().map(|d| d.cards.len() as u32).sum();
    let mut all = decks;
    all.extend(wiki.decks);
    Ok(InsightSources {
        decks: all,
        deck_files,
        deck_cards,
        wiki_notes: wiki.notes,
        wiki_cards,
        wiki_root,
        wiki_folders: wiki.folders,
        wiki_scope,
        warnings,
    })
}

/// 무엇이 있는가. 게이트가 열리기 전에 이것부터 본다.
#[tauri::command]
pub async fn insight_availability(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<InsightAvailability, String> {
    let pool = pool_of(&state)?;
    let enabled = insight_on(&pool).await;
    let sources = insight_sources(&app, &pool).await?;
    Ok(InsightAvailability {
        cards: sources.deck_cards + sources.wiki_cards,
        decks: sources.deck_files,
        deck_cards: sources.deck_cards,
        wiki_notes: sources.wiki_notes,
        wiki_cards: sources.wiki_cards,
        wiki_root: sources.wiki_root,
        wiki_folders: sources.wiki_folders,
        wiki_scope: sources.wiki_scope,
        enabled,
        warnings: sources.warnings,
    })
}

/// 다음 카드. 낸 카드는 최근 목록에 기록해 연달아 다시 나오지 않게 한다.
#[tauri::command]
pub async fn insight_next(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<crate::insightdeck::deck::InsightCard>, String> {
    let pool = pool_of(&state)?;
    let sources = insight_sources(&app, &pool).await?;

    let recent = insight_recent(&pool).await;
    // seed는 시각이다 — 같은 초에 두 번 부르면 같은 카드가 나오지만, 그때는 최근 목록이
    // 이미 갱신돼 있어 다음 호출이 다른 것을 고른다.
    let picked =
        crate::insightdeck::serve::pick_next(&sources.decks, &recent, now() as u64).cloned();

    if let Some(card) = &picked {
        let mut next = vec![card.key.clone()];
        next.extend(recent.into_iter().filter(|k| k != &card.key));
        next.truncate(crate::insightdeck::serve::RECENT_WINDOW);
        let _ = db::set_setting(&pool, INSIGHT_RECENT_KEY, &next.join("\n")).await;
    }
    Ok(picked)
}

/// 카드로 쓸 지식창고 폴더 범위를 저장한다. `None`은 "전체"로 되돌리는 것이고, 빈 목록은
/// "지식창고 카드 없음"이다 — 설정 화면은 체크박스를 다 끄면 빈 목록을 보낸다.
#[tauri::command]
pub async fn insight_wiki_folders_set(
    state: State<'_, AppState>,
    folders: Option<Vec<String>>,
) -> Result<(), String> {
    let pool = pool_of(&state)?;
    let folders = folders.map(|list| {
        let mut list: Vec<String> = list
            .into_iter()
            .map(|f| f.trim().trim_end_matches('/').to_string())
            .filter(|f| !f.is_empty())
            .collect();
        list.sort();
        list.dedup();
        list
    });
    let raw = serde_json::to_string(&folders).map_err(|e| e.to_string())?;
    db::set_setting(&pool, INSIGHT_WIKI_FOLDERS_KEY, &raw)
        .await
        .map_err(|e| e.to_string())
}

/// 대기 인사이트 토글의 유효값. 미설정 시 켜짐.
async fn insight_on(pool: &SqlitePool) -> bool {
    db::get_setting(pool, "insight_on")
        .await
        .ok()
        .flatten()
        .as_deref()
        != Some("false")
}

#[tauri::command]
pub async fn insight_enabled_get(state: State<'_, AppState>) -> Result<bool, String> {
    let pool = pool_of(&state)?;
    Ok(insight_on(&pool).await)
}

#[tauri::command]
pub async fn insight_enabled_set(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    let pool = pool_of(&state)?;
    db::set_setting(&pool, "insight_on", if on { "true" } else { "false" })
        .await
        .map_err(|e| e.to_string())
}

/// inbox 절대 경로 — DB와 같은 앱 데이터 디렉터리 밑이다(워크트리 아님).
fn quiz_inbox_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(crate::quiz::generate::INBOX_SUBDIR)
}

