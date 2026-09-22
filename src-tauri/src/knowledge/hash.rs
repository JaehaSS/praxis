//! 문서 변경 감지용 해시.

use sha2::{Digest, Sha256};

/// 본문의 내용 해시. 같으면 재청킹·재임베딩을 건너뛴다.
///
/// 정규화는 **개행과 후행 공백만** 건드린다. 그 이상(공백 축약·대소문자·구두점)을
/// 정규화하면 실제 편집을 "변경 없음"으로 오판해 낡은 색인이 영구히 남는다.
/// 재처리는 싸고 누락은 비싸다.
pub fn content_hash(body: &str) -> String {
    let normalized = body.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(normalized.trim_end().as_bytes());
    format!("{:x}", hasher.finalize())
}
