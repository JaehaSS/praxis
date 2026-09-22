//! vault 스캔 검증.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::knowledge::source::obsidian::{scan_vault, to_external_id, VaultConfig};

static VAULT_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 신규 의존성(tempfile)을 넣지 않으려고 직접 만든다 — 설계상 "신규 의존성 없음"이 제약.
fn temp_vault(files: &[(&str, &str)]) -> PathBuf {
    let n = VAULT_COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = crate::testtmp::dir().join(format!(
        "praxis-vault-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    for (rel, body) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
    }
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn excludes_obsidian_internals_and_non_markdown() {
    let root = temp_vault(&[
        ("note.md", "본문"),
        (".obsidian/workspace.json", "{}"),
        (".trash/deleted.md", "삭제됨"),
        ("image.png", "PNG"),
    ]);
    let docs = scan_vault(&VaultConfig::new(&root)).unwrap();
    let ids: Vec<&str> = docs.iter().map(|d| d.external_id.as_str()).collect();
    assert_eq!(ids, vec!["note.md"]);
}

#[test]
fn nested_directories_are_scanned() {
    let root = temp_vault(&[("a/b/deep.md", "깊은 본문"), ("top.md", "위")]);
    let docs = scan_vault(&VaultConfig::new(&root)).unwrap();
    let ids: Vec<&str> = docs.iter().map(|d| d.external_id.as_str()).collect();
    assert_eq!(ids, vec!["a/b/deep.md", "top.md"]);
}

#[test]
fn user_exclude_globs_apply_to_files_and_directories() {
    let root = temp_vault(&[
        ("keep.md", "유지"),
        ("Templates/skip.md", "건너뜀"),
        ("draw.excalidraw.md", "그림"),
    ]);
    let cfg = VaultConfig {
        root: root.clone(),
        exclude: vec!["Templates/**".into(), "*.excalidraw.md".into()],
        embed_exclude: Vec::new(),
    };
    let docs = scan_vault(&cfg).unwrap();
    let ids: Vec<&str> = docs.iter().map(|d| d.external_id.as_str()).collect();
    assert_eq!(ids, vec!["keep.md"]);
}

#[test]
fn external_id_uses_forward_slashes_on_every_platform() {
    // UNIQUE 키다. Windows에서 `\`가 들어가면 같은 노트가 OS마다 다른 문서가 되어
    // 재동기화 때 vault 전체가 신규로 잡힌다.
    let root = Path::new("/tmp/vault");
    let path = root.join("sub").join("note.md");
    assert_eq!(to_external_id(&path, root).as_deref(), Some("sub/note.md"));
    assert!(!to_external_id(&path, root).unwrap().contains('\\'));
}

#[test]
fn symlink_is_not_followed() {
    // `~/Documents`처럼 넓은 경로를 vault로 고르면 순환 링크를 밟을 확률이 실재한다.
    let root = temp_vault(&[("real.md", "본문")]);
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(&root, root.join("loop"));
    }
    let docs = scan_vault(&VaultConfig::new(&root)).unwrap();
    assert_eq!(docs.len(), 1, "심볼릭 링크를 따라갔다");
}

#[test]
fn scan_order_is_deterministic() {
    // 파일시스템 순회 순서는 플랫폼마다 다르다. 정렬하지 않으면 동기화 로그와
    // 테스트가 실행마다 달라진다.
    let root = temp_vault(&[("z.md", "z"), ("a.md", "a"), ("m/n.md", "n")]);
    let first = scan_vault(&VaultConfig::new(&root)).unwrap();
    let second = scan_vault(&VaultConfig::new(&root)).unwrap();
    let ids = |v: &[crate::knowledge::graph::Document]| {
        v.iter().map(|d| d.external_id.clone()).collect::<Vec<_>>()
    };
    assert_eq!(ids(&first), ids(&second));
    assert_eq!(ids(&first), vec!["a.md", "m/n.md", "z.md"]);
}

#[test]
fn title_comes_from_the_file_stem() {
    let root = temp_vault(&[("폴더/한글 노트.md", "본문")]);
    let docs = scan_vault(&VaultConfig::new(&root)).unwrap();
    assert_eq!(docs[0].title, "한글 노트");
    assert_eq!(docs[0].external_id, "폴더/한글 노트.md");
}

#[test]
fn missing_vault_yields_empty_not_error() {
    // vault 경로가 지워졌거나 외장 디스크가 빠진 상태에서 동기화가 터지면 안 된다.
    let cfg = VaultConfig::new("/nonexistent/praxis/vault");
    assert!(scan_vault(&cfg).unwrap().is_empty());
}
