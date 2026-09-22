//! 로컬 텍스트 임베딩 (fastembed, BGE-small-en-v1.5, 384차원).
//! 최초 사용 시 모델 다운로드(~130MB)+로드 — 이후 프로세스 전역 캐시.
//! 호출자는 best-effort로 사용(실패 시 FTS로 폴백).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

static MODEL: OnceLock<TextEmbedding> = OnceLock::new();
/// 초기화 직렬화 락 — 워밍업(lib.rs)과 첫 작업 생성이 겹칠 때 ~130MB 모델을 두 스레드가
/// 동시에 다운로드/로드하는 것을 막는다. 실패는 캐시하지 않아(일시 네트워크 오류) 재시도 가능.
static INIT: Mutex<()> = Mutex::new(());
/// 모델 캐시 위치. 미주입이면 fastembed 기본값(CWD 상대 `.fastembed_cache`)이라
/// 실행 위치가 바뀔 때마다 모델을 다시 내려받는다.
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 캐시 디렉터리 주입 — **1회만 유효**하다(두 번째 호출은 무시). 모델이 이미 로드된 뒤에
/// 경로를 바꾸면 같은 프로세스가 두 위치를 쓰게 되므로 바꿀 수 있게 두지 않는다.
pub fn set_cache_dir(dir: PathBuf) {
    let _ = CACHE_DIR.set(dir);
}

fn init_options(model: EmbeddingModel) -> InitOptions {
    let options = InitOptions::new(model);
    match CACHE_DIR.get() {
        Some(dir) => options.with_cache_dir(dir.clone()),
        None => options,
    }
}

fn model() -> anyhow::Result<&'static TextEmbedding> {
    if let Some(m) = MODEL.get() {
        return Ok(m);
    }
    let _guard = INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = MODEL.get() {
        return Ok(m); // 락 대기 중 다른 스레드가 로드 완료
    }
    let m = TextEmbedding::try_new(init_options(EmbeddingModel::BGESmallENV15))?;
    let _ = MODEL.set(m);
    Ok(MODEL.get().expect("model set"))
}

/// 텍스트 1건을 384차원 벡터로 임베딩.
pub fn embed(text: &str) -> anyhow::Result<Vec<f32>> {
    let m = model()?;
    let mut out = m.embed(vec![text], None)?;
    out.pop().ok_or_else(|| anyhow::anyhow!("임베딩 결과 없음"))
}

/// 모델이 **이미 로드돼 있을 때만** 임베딩한다. 로드 전이면 즉시 None —
/// 사용자가 기다리는 생성 경로가 ~130MB 모델 로드를 대기 시간으로 떠안지 않게 한다.
/// `INIT` 락을 잡지 않는다: 워밍업과 겹쳐도 `OnceLock` 읽기뿐이라 오답이 없다.
pub fn embed_if_ready(text: &str) -> Option<Vec<f32>> {
    let m = MODEL.get()?;
    m.embed(vec![text], None).ok()?.pop()
}

// ── 지식 그래프용 다국어 임베딩 (설계 0020 DR-2) ──
//
// 위 `embed()`(BGE-small-**EN**)와 **별도 모델**이다. 기존 모델은 영어 전용이라
// 한국어 노트·메일에서는 시맨틱 검색이 사실상 작동하지 않는다 — 그런데 증상이
// "그냥 잘 안 찾네"로만 보여 원인이 드러나지 않는다.
//
// 두 벡터 공간을 절대 섞지 않는다. `memories`는 `embed()`, `knowledge_chunks`는
// 여기 함수들만 쓴다. 차원은 둘 다 384라 BLOB 인코딩·`cosine`은 공유할 수 있다.

/// `knowledge_chunks.embed_model`에 기록하는 값. 모델을 바꾸면 이 값이 다른 행이
/// 재임베딩 대상이 된다.
pub const KNOWLEDGE_MODEL: &str = "multilingual-e5-small";

static MULTILINGUAL: OnceLock<TextEmbedding> = OnceLock::new();

/// E5 계열은 **비대칭** 모델이라 질의와 문서에 서로 다른 접두사가 필요하다.
/// 접두사를 이 모듈 안에 가둬 호출자가 잊을 수 없게 한다 — 누락은 컴파일도 테스트도
/// 통과하면서 검색 품질만 떨어뜨리는, 가장 발견하기 어려운 종류의 버그다.
const QUERY_PREFIX: &str = "query: ";
const PASSAGE_PREFIX: &str = "passage: ";

fn multilingual_model() -> anyhow::Result<&'static TextEmbedding> {
    if let Some(m) = MULTILINGUAL.get() {
        return Ok(m);
    }
    // INIT을 `model()`과 공유한다 — 두 모델(각 ~120MB)이 동시에 내려오면 대역폭을
    // 나눠 쓰며 둘 다 느려진다. 직렬화가 낫다.
    let _guard = INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(m) = MULTILINGUAL.get() {
        return Ok(m);
    }
    let m = TextEmbedding::try_new(init_options(EmbeddingModel::MultilingualE5Small))?;
    let _ = MULTILINGUAL.set(m);
    Ok(MULTILINGUAL.get().expect("multilingual model set"))
}

/// 색인용 — 문서 조각을 벡터로. `embed_query`와 반드시 짝으로 쓴다.
pub fn embed_passage(text: &str) -> anyhow::Result<Vec<f32>> {
    let mut out = embed_passages(std::slice::from_ref(&text.to_string()))?;
    out.pop().ok_or_else(|| anyhow::anyhow!("임베딩 결과 없음"))
}

/// 색인용 배치 — 백필 성능의 핵심이다. 건별 호출은 3.9만 청크에서 ~10분이 걸린다.
pub fn embed_passages(texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let m = multilingual_model()?;
    let prefixed: Vec<String> = texts.iter().map(|t| format!("{PASSAGE_PREFIX}{t}")).collect();
    m.embed(prefixed, None)
}

/// 질의용 — 검색어를 벡터로.
pub fn embed_query(text: &str) -> anyhow::Result<Vec<f32>> {
    let m = multilingual_model()?;
    let mut out = m.embed(vec![format!("{QUERY_PREFIX}{text}")], None)?;
    out.pop().ok_or_else(|| anyhow::anyhow!("임베딩 결과 없음"))
}

#[cfg(test)]
mod tests;
