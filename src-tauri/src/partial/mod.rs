//! Atomic hunk-level approval with fail-closed rollback.

use crate::diffmodel::DiffHunk;

mod operations;
mod patch;
mod store;

pub(crate) use operations::apply_forward_patch;
pub use operations::{apply, rollback};
pub use patch::compose_patch;
pub use store::{clear_checkpoint, get_checkpoint, migrate, save_checkpoint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialError {
    HunkNotFound(Vec<String>),
    ProtectedHunkRejected(Vec<String>),
    CommittedHunkRejected(Vec<String>),
    ApplyConflict(Vec<String>),
    NoCheckpoint,
    Git(String),
}

impl std::fmt::Display for PartialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HunkNotFound(ids) => {
                write!(formatter, "존재하지 않는 hunk id: {}", ids.join(","))
            }
            Self::CommittedHunkRejected(ids) => write!(
                formatter,
                "이미 커밋된 hunk는 부분 적용 대상이 아닙니다: {}",
                ids.join(",")
            ),
            Self::ProtectedHunkRejected(ids) => write!(
                formatter,
                "protected hunk는 부분 적용으로 유지할 수 없습니다: {}",
                ids.join(",")
            ),
            Self::ApplyConflict(ids) => write!(
                formatter,
                "역패치 적용 충돌 — 상태는 체크포인트로 원복되었습니다. 실패 hunk: {}",
                ids.join(",")
            ),
            Self::NoCheckpoint => write!(formatter, "되돌릴 부분 적용 체크포인트가 없습니다"),
            Self::Git(message) => write!(formatter, "git 실행 실패: {message}"),
        }
    }
}

impl std::error::Error for PartialError {}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub checkpoint: String,
    pub kept_hunk_ids: Vec<String>,
    pub discarded_hunk_ids: Vec<String>,
}

pub fn reject_protected(selected: &[&DiffHunk]) -> Result<(), PartialError> {
    let ids = selected
        .iter()
        .filter(|hunk| hunk.protected)
        .map(|hunk| hunk.id.clone())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    Err(PartialError::ProtectedHunkRejected(ids))
}

/// 커밋된 hunk의 "유지 선택"을 거부한다.
///
/// 폐기 쪽은 이 함수가 막지 못한다 — `apply`가 **비선택** hunk를 역패치하므로, 그냥 고르지
/// 않는 것만으로 되돌아간다. 그래서 `apply`가 비선택 집합에서도 커밋된 hunk를 빼야 한다.
/// 둘 중 하나만 있으면 반쪽이다.
pub fn reject_committed(selected: &[&DiffHunk]) -> Result<(), PartialError> {
    let ids = selected
        .iter()
        .filter(|hunk| hunk.committed)
        .map(|hunk| hunk.id.clone())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    Err(PartialError::CommittedHunkRejected(ids))
}

#[cfg(test)]
mod tests;
