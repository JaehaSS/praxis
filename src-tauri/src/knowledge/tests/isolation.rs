//! 로컬 경계 강제 — 지식 그래프는 Windows 데스크톱 안에만 존재한다(설계 0020 DR-6).

use std::path::Path;

/// Runner·모바일에 지식 그래프를 노출하지 않는다.
///
/// 라우트를 "만들지 않는 것"은 오늘은 맞지만 6개월 뒤 "모바일에서도 보면 좋겠다"는
/// 한 줄 커밋에 조용히 무너진다. 경계는 깨지면 실패하는 형태로 적어야 한다.
#[test]
fn runner_does_not_reference_knowledge_module() {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runner");
    let mut offenders = Vec::new();
    visit_rs_files(&runner, &mut |path, body| {
        if body.contains("crate::knowledge") || body.contains("knowledge_nodes") {
            offenders.push(path.display().to_string());
        }
    });
    assert!(
        offenders.is_empty(),
        "Runner가 지식 그래프를 참조한다 — 로컬 경계 위반: {offenders:?}"
    );
}

/// `quiz`는 도메인 문제를 만들려고 지식 그래프를 참조한다(설계 0044). 그래서 Runner가 `quiz`를
/// 참조하면 그래프가 **간접적으로** 새어 나간다 — 위 검사는 직접 참조만 보므로 이 경로를 놓친다.
#[test]
fn runner_does_not_reference_quiz_module() {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runner");
    let mut offenders = Vec::new();
    visit_rs_files(&runner, &mut |path, body| {
        if body.contains("crate::quiz") || body.contains("quiz_items") {
            offenders.push(path.display().to_string());
        }
    });
    assert!(
        offenders.is_empty(),
        "Runner가 퀴즈 모듈을 참조한다 — 지식 그래프가 간접 노출된다: {offenders:?}"
    );
}

/// 위 검사가 실제로 파일을 읽고 있는지 확인한다.
/// 경로가 틀려 0개 파일을 훑으면 검사는 영원히 통과하고 경계는 보호되지 않는다.
#[test]
fn the_boundary_check_actually_scans_files() {
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runner");
    let mut seen = 0usize;
    visit_rs_files(&runner, &mut |_, _| seen += 1);
    assert!(seen > 10, "runner에서 {seen}개 파일만 읽었다 — 경로가 틀렸다");
}

fn visit_rs_files(dir: &Path, visit: &mut impl FnMut(&Path, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rs_files(&path, visit);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(body) = std::fs::read_to_string(&path) {
                visit(&path, &body);
            }
        }
    }
}
