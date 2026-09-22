//! 테스트 산출물을 담을 **프로세스 전용 임시 루트**.
//!
//! 테스트가 `std::env::temp_dir()`에 `{pid}` 기반 이름으로 DB를 만들면 두 가지가 무너진다.
//!
//! 1. macOS는 pid를 금세 재사용한다. 앞선 실행이 남긴 `-wal`/`-shm`을 같은 경로에서 만나면
//!    sqlite가 그 WAL을 **복구해 옛 스키마를 되살린다** — `CREATE TABLE IF NOT EXISTS`가
//!    no-op이 되고 뒤의 `ALTER`가 스키마를 바꿔, 그 창에서 준비된 조회가 없는 컬럼을 읽는다
//!    (이슈 #144·#153).
//! 2. 본체만 지우는 정리는 WAL 부산물을 남긴다. 실제로 임시 디렉터리에 수만 개가 쌓여 있었다.
//!
//! 루트 하나를 프로세스마다 새로 파고 그 아래에 전부 두면 둘 다 사라진다 — 경로가 과거 실행과
//! 겹치지 않고, 지울 것도 디렉터리 하나다. 격리의 근거를 pid에 두면 pid가 돌아오는 순간
//! 격리가 사라지므로, 이름에 나노초를 섞는다.

// support 모듈을 포함하는 부모 파일은 이 모듈을 선언해야 하지만(그 support가 `super::`로
// 부른다), 자기 본문에서는 안 쓸 수 있다. 그 자리에서 dead_code 경고가 나는 것을 막는다.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 루트 이름의 접두사. 청소는 **이 접두사를 가진 것만** 건드린다 — 임시 디렉터리에는 남의
/// 파일도 산다.
const ROOT_PREFIX: &str = "praxis-testroot-";

/// 이보다 오래 손대지 않은 루트는 끝난 실행의 것으로 본다.
///
/// 전체 스위트 한 번이 3분 남짓이므로 두 시간이면 지금 도는 형제를 지울 위험이 없다.
/// 테스트 바이너리에는 종료 훅이 없어 자기 루트를 스스로 치울 수 없다 — 대신 다음 실행이
/// 앞선 실행의 것을 치운다.
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
