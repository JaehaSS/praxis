//! 유닛 테스트 산출물을 담을 **프로세스 전용 임시 루트**.
//!
//! 테스트가 `std::env::temp_dir()`에 `{pid}` 기반 이름으로 DB를 만들면 두 가지가 무너진다.
//!
//! 1. macOS는 pid를 금세 재사용한다. 앞선 실행이 남긴 `-wal`/`-shm`을 같은 경로에서 만나면
//!    sqlite가 그 WAL을 **복구해 옛 스키마를 되살린다** — `CREATE TABLE IF NOT EXISTS`가
//!    no-op이 되고 뒤의 `ALTER`가 스키마를 바꿔, 그 창에서 준비된 조회가 없는 컬럼을 읽는다
//!    (이슈 #144·#153).
//! 2. 본체만 지우는 정리는 WAL 부산물을 남긴다. 실제로 임시 디렉터리에 수만 개가 쌓여 있었다.
//!
//! 통합 테스트에는 같은 일을 하는 짝이 있다(`tests/support/temp_root.rs`). 크레이트 경계가
//! 달라 한 벌로 합칠 수 없으므로 **접두사와 나이 기준을 같은 값으로 맞춰 둔다** — 한쪽만
//! 고치면 청소가 절반만 돈다.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 루트 이름의 접두사. 청소는 **이 접두사를 가진 것만** 건드린다 — 임시 디렉터리에는 남의
/// 파일도 산다.
const ROOT_PREFIX: &str = "praxis-testroot-";

/// 이보다 오래 손대지 않은 루트는 끝난 실행의 것으로 본다.
///
/// 테스트 바이너리에는 종료 훅이 없어 자기 루트를 스스로 치울 수 없다 — 대신 다음 실행이
/// 앞선 실행의 것을 치운다. 전체 스위트 한 번이 3분 남짓이므로 두 시간이면 지금 도는 형제를
/// 지울 위험이 없다.
const STALE_AFTER: Duration = Duration::from_secs(2 * 60 * 60);

/// 이 테스트 프로세스 전용 임시 루트. 최초 호출에서 만들고, 그 김에 옛 루트를 치운다.
///
/// `std::env::temp_dir()`가 있던 자리를 그대로 대신하도록 `PathBuf`를 돌려준다.
pub fn dir() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let parent = std::env::temp_dir();
        sweep_old_roots(&parent);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = parent.join(format!("{ROOT_PREFIX}{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).expect("테스트 임시 루트를 만들지 못했습니다");
        root
    })
    .clone()
}

/// 나이로 걸러 옛 루트를 지운다.
///
/// 실패는 무시한다 — 다른 프로세스가 같은 순간에 치우고 있을 수 있고, 청소가 못 되는 것이
/// 테스트를 세울 이유는 되지 않는다.
fn sweep_old_roots(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(ROOT_PREFIX) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 청소는 **남의 것과 지금 도는 형제를 건드리지 않는다.**
    ///
    /// 위험한 방향은 이쪽이다 — 못 지우면 디스크만 차지만, 과잉 삭제는 동시에 도는 테스트의
    /// DB를 걷어내 원인을 알 수 없는 실패를 만든다. 그 실패는 청소를 의심할 단서를 남기지 않는다.
    #[test]
    fn sweeping_spares_foreign_entries_and_fresh_siblings() {
        let parent = dir().join("sweep-case");
        std::fs::create_dir_all(&parent).unwrap();
        let foreign = parent.join("someone-elses-dir");
        let fresh = parent.join(format!("{ROOT_PREFIX}999999-1"));
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::create_dir_all(&fresh).unwrap();

        sweep_old_roots(&parent);

        assert!(foreign.exists(), "접두사가 다른 항목은 건드리지 않는다");
        assert!(fresh.exists(), "갓 만든 형제는 지금 도는 실행의 것일 수 있다");
    }
}
