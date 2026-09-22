use std::path::{Path, PathBuf};

impl super::HarnessName {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "workflow-harness" => Some(Self::WorkflowHarness),
            "loop-engineering" => Some(Self::LoopEngineering),
            _ => None,
        }
    }
}

/// 작업 이력이 있는 프로젝트의 canonical root와만 정확히 일치시킨다.
///
/// 기존 봇 경로의 하위 경로 허용은 실행 보안 정책이며, 경험 읽기의 권한 근거가 아니다.
pub fn registered_root(repo: &str, known_repos: &[String]) -> Option<PathBuf> {
    let target = Path::new(repo).canonicalize().ok()?;
    known_repos.iter().find_map(|known| {
        Path::new(known)
            .canonicalize()
            .ok()
            .filter(|canonical| canonical == &target)
            .map(|_| target.clone())
    })
}
