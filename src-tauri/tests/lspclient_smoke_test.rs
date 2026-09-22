//! 실제 언어 서버와의 왕복 스모크 — 프레이밍·핸드셰이크·좌표 변환이 한 줄로 이어지는지 본다.
//!
//! 서버 설치에 의존하므로 `#[ignore]`다. 유닛 테스트는 프로토콜 조각만 검증하니,
//! 붙는지는 여기서 확인한다:
//!   cargo test --test lspclient_smoke_test -- --ignored --nocapture

use std::path::PathBuf;

use praxis_lib::lspclient::{GotoKind, LspPool};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri의 부모가 레포 루트")
        .to_path_buf()
}

/// 파일에서 `needle`이 처음 나오는 (line, column)을 Monaco 좌표(1-based)로 찾는다.
/// 줄 번호를 테스트에 박아두면 대상 파일이 한 줄만 밀려도 깨지므로 런타임에 계산한다.
fn locate(text: &str, line_contains: &str, needle: &str) -> (u32, u32) {
    let (index, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(line_contains))
        .unwrap_or_else(|| panic!("'{line_contains}' 를 담은 줄을 찾지 못했습니다"));
    let column = line.find(needle).expect("needle이 그 줄에 있어야 한다");
    (index as u32 + 1, column as u32 + 1)
}

#[tokio::test]
#[ignore = "typescript-language-server 설치 필요"]
async fn typescript_definition_lands_on_the_declaration() {
    let root = repo_root();
    let rel = "src/lib/lsp.ts";
    let text = std::fs::read_to_string(root.join(rel)).expect("대상 파일 읽기");

    // resolveOutcome 본문의 dedupeTargets 호출부 → 같은 파일의 dedupeTargets 선언으로 가야 한다.
    let (line, column) = locate(&text, "const unique = dedupeTargets(", "dedupeTargets");
    let pool = LspPool::default();

    let targets = pool
        .goto(1, &root, rel, &text, line, column, GotoKind::Definition)
        .await
        .expect("정의 조회 성공");

    let expected_line = locate(&text, "export function dedupeTargets", "dedupeTargets").0;
    println!("{line}:{column} → {targets:?} (기대 줄 {expected_line})");
    assert!(
        targets
            .iter()
            .any(|t| t.path.as_deref() == Some(rel) && t.line == expected_line),
        "선언 줄({expected_line})을 가리켜야 하는데 {targets:?}",
    );
}

#[tokio::test]
#[ignore = "typescript-language-server 설치 필요"]
async fn unsaved_edit_is_visible_to_the_server() {
    let root = repo_root();
    let rel = "src/lib/lsp.ts";
    let disk = std::fs::read_to_string(root.join(rel)).expect("대상 파일 읽기");

    // 디스크에 없는 함수를 버퍼에만 추가하고, 그 함수의 호출부에서 정의를 묻는다.
    // 서버가 저장본만 본다면 여기서 결과가 비어 실패한다.
    let edited = format!("{disk}\nfunction 임시함수() {{ return 1; }}\nconst 사용 = 임시함수();\n");
    let (line, column) = locate(&edited, "const 사용 = 임시함수()", "임시함수()");
    let expected_line = locate(&edited, "function 임시함수()", "임시함수").0;

    let pool = LspPool::default();
    let targets = pool
        .goto(2, &root, rel, &edited, line, column, GotoKind::Definition)
        .await
        .expect("정의 조회 성공");

    println!("미저장 편집 {line}:{column} → {targets:?}");
    assert!(
        targets.iter().any(|t| t.line == expected_line),
        "버퍼에만 있는 선언({expected_line})을 찾아야 하는데 {targets:?}",
    );
}

#[tokio::test]
#[ignore = "rust-analyzer 설치 필요 (rustup component add rust-analyzer). 첫 인덱싱이 느리면 타임아웃될 수 있다"]
async fn rust_definition_lands_on_the_declaration() {
    let root = repo_root();
    let rel = "src/lspclient/protocol.rs";
    let text = std::fs::read_to_string(root.join("src-tauri").join(rel)).expect("대상 파일 읽기");
    let (line, column) = locate(&text, "let mut out = format!(\"Content-Length", "format!");

    let pool = LspPool::default();
    // 워크트리 루트는 Cargo.toml이 있는 src-tauri다.
    let result = pool
        .goto(
            4,
            &root.join("src-tauri"),
            rel,
            &text,
            line,
            column,
            GotoKind::Definition,
        )
        .await;

    // 서버가 없거나 인덱싱이 안 끝났으면 사유가 그대로 보여야 한다 — 빈손 실패는 진단이 안 된다.
    match result {
        Ok(targets) => println!("rust 정의 → {targets:?}"),
        Err(reason) => panic!("정의 조회 실패(사유는 사용자에게 그대로 노출된다):\n{reason}"),
    }
}

#[tokio::test]
#[ignore = "typescript-language-server 설치 필요"]
async fn references_finds_call_sites() {
    let root = repo_root();
    let rel = "src/lib/lsp.ts";
    let text = std::fs::read_to_string(root.join(rel)).expect("대상 파일 읽기");
    let (line, column) = locate(&text, "export function dedupeTargets", "dedupeTargets");

    let pool = LspPool::default();
    let targets = pool
        .goto(3, &root, rel, &text, line, column, GotoKind::References)
        .await
        .expect("사용처 조회 성공");

    println!("사용처 → {targets:?}");
    assert!(
        !targets.is_empty(),
        "resolveOutcome 등에서 쓰이므로 비어선 안 된다"
    );
    assert!(
        targets
            .iter()
            .all(|t| t.line != line || t.path.as_deref() != Some(rel)),
        "includeDeclaration=false 이므로 선언 자신은 빠져야 한다: {targets:?}",
    );
}
