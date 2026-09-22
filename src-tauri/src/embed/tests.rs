//! 다국어 임베딩 검증.
//!
//! 최초 실행 시 모델 ~120MB를 내려받는다. 네트워크가 없으면 실패한다 —
//! 그래도 `#[ignore]`를 붙이지 않는다. E5 접두사 누락이나 모델 교체 실수는
//! 조용한 품질 저하로만 나타나고, 이 테스트가 그것을 잡는 유일한 장치다.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use fastembed::{EmbeddingModel, InitOptions};

use super::{embed_if_ready, embed_passage, embed_passages, embed_query, init_options, set_cache_dir, INIT};
use crate::memory::cosine;

#[test]
fn dimension_matches_the_existing_blob_format() {
    // 384가 아니면 memories와 같은 f32 LE 인코딩·cosine을 공유할 수 없다.
    assert_eq!(embed_passage("치수 확인").unwrap().len(), 384);
}

#[test]
fn korean_query_finds_the_related_passage() {
    // 영어 전용 모델(BGE-small-EN)이면 이 구분이 무너진다 — DR-2의 핵심 근거.
    let q = embed_query("지식 그래프를 어떻게 만들지").unwrap();
    let hit = embed_passage("로컬 지식 그래프 설계와 RAG 검색 파이프라인").unwrap();
    let miss = embed_passage("오늘 점심으로 김치찌개를 먹었다").unwrap();
    let (s_hit, s_miss) = (cosine(&q, &hit), cosine(&q, &miss));
    assert!(
        s_hit > s_miss,
        "관련 문서가 더 가깝지 않다: hit={s_hit:.3} miss={s_miss:.3}"
    );
}

#[test]
fn batch_matches_single_embedding() {
    // 배치 경로가 접두사를 다르게 붙이면 색인과 질의의 벡터 공간이 어긋난다.
    // 백필은 배치로, 재색인은 단건으로 도는 경로가 섞이면 조용히 깨진다.
    let single = embed_passage("배치 일관성 확인 문장").unwrap();
    let batch = embed_passages(&["배치 일관성 확인 문장".to_string()]).unwrap();
    assert_eq!(batch.len(), 1);
    assert!(
        cosine(&single, &batch[0]) > 0.999,
        "단건과 배치 결과가 다르다"
    );
}

#[test]
fn empty_batch_is_not_an_error() {
    // 변경분이 없는 동기화에서 빈 배치가 정상적으로 들어온다.
    assert!(embed_passages(&[]).unwrap().is_empty());
}

#[test]
fn cache_dir_is_injected_once() {
    // 기본값과 같은 경로를 주입한다 — 같은 테스트 바이너리의 모델 로드 테스트가 다른
    // 디렉터리를 보고 모델을 다시 내려받는 일을 만들지 않기 위해서다.
    let default = InitOptions::new(EmbeddingModel::BGESmallENV15).cache_dir;
    set_cache_dir(default.clone());
    set_cache_dir(PathBuf::from("/tmp/praxis-embed-cache-ignored"));
    assert_eq!(
        init_options(EmbeddingModel::BGESmallENV15).cache_dir,
        default,
        "두 번째 주입이 경로를 바꿨다"
    );
}

#[test]
fn embed_if_ready_does_not_wait_for_initialization() {
    // 워밍업이 INIT을 쥔 채 ~130MB를 내려받는 동안 생성 경로가 여기서 멎으면
    // 이 함수의 존재 이유가 사라진다.
    let holder = std::thread::spawn(|| {
        let _guard = INIT.lock().unwrap_or_else(|e| e.into_inner());
        std::thread::sleep(Duration::from_millis(500));
    });
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    let _ = embed_if_ready("락 경합 확인");
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "INIT 락을 기다렸다: {:?}",
        started.elapsed()
    );
    holder.join().unwrap();
}
