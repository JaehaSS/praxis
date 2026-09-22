use std::path::{Path, PathBuf};

use super::*;

fn roots(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = crate::testtmp::dir().join(name);
    let home = root.join("home");
    let task = root.join("task");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&task).unwrap();
    (root, home, task)
}

fn open_spy(path: &str, task: &Path, home: &Path) -> Result<PathBuf, String> {
    let mut seen = None;
    open_verified(path, task, home, |file| {
        seen = Some(file.to_path_buf());
        Ok(())
    })?;
    Ok(seen.unwrap())
}

fn assert_denied_without_open(path: &str, task: &Path, home: &Path) {
    let mut opened = false;
    let error = open_verified(path, task, home, |_| {
        opened = true;
        Ok(())
    })
    .unwrap_err();
    assert!(error.starts_with("local_file_denied:"), "{error}");
    assert!(!opened, "opener must not receive denied paths");
}

#[test]
fn home_document_with_korean_and_space_reaches_canonical_opener_path() {
    let (_, home, task) = roots("local-file-home-document");
    let document = home.join("Documents/한글 문서.md");
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "ok").unwrap();
    assert_eq!(
        open_spy(document.to_str().unwrap(), &task, &home).unwrap(),
        document.canonicalize().unwrap()
    );
    let content = read_verified(document.to_str().unwrap(), &task, &home).unwrap();
    assert_eq!(content.kind, crate::fsapi::FileKind::Text);
    assert_eq!(content.content, "ok");
    let large = home.join("Documents/large.md");
    std::fs::write(&large, vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    let content = read_verified(large.to_str().unwrap(), &task, &home).unwrap();
    assert_eq!(content.kind, crate::fsapi::FileKind::TooLarge);
    assert!(content.content.is_empty());
}

#[test]
fn task_relative_and_absolute_paths_are_opened() {
    let (_, home, task) = roots("local-file-task-paths");
    let document = task.join("notes/plan.md");
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "ok").unwrap();
    let expected = document.canonicalize().unwrap();
    assert_eq!(open_spy("notes/plan.md", &task, &home).unwrap(), expected);
    assert_eq!(
        open_spy(document.to_str().unwrap(), &task, &home).unwrap(),
        expected
    );
    let content = read_verified("notes/plan.md", &task, &home).unwrap();
    assert_eq!(content.content, "ok");
}

#[test]
fn outside_prefix_traversal_and_control_inputs_are_rejected() {
    let (root, home, task) = roots("local-file-invalid-inputs");
    let sibling = root.join("task-other/note.md");
    std::fs::create_dir_all(sibling.parent().unwrap()).unwrap();
    std::fs::write(&sibling, "ok").unwrap();
    assert_denied_without_open(sibling.to_str().unwrap(), &task, &home);
    assert!(read_verified(sibling.to_str().unwrap(), &task, &home)
        .unwrap_err()
        .starts_with("local_file_denied:"));
    for path in ["../escape.md", "notes/../../escape.md", "note\0.md"] {
        assert!(
            open_spy(path, &task, &home)
                .unwrap_err()
                .starts_with("local_file_"),
            "{path} must be rejected"
        );
        assert!(
            read_verified(path, &task, &home)
                .unwrap_err()
                .starts_with("local_file_"),
            "{path} must be rejected"
        );
    }
}

#[cfg(unix)]
#[test]
fn leaf_and_ancestor_symlinks_are_denied_without_home_fallback() {
    use std::os::unix::fs::symlink;

    let (_, home, _) = roots("local-file-symlinks");
    let task = home.join("worktree");
    std::fs::create_dir_all(&task).unwrap();
    let document = home.join("Documents/a.md");
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(&document, "ok").unwrap();
    symlink(&document, task.join("out")).unwrap();
    let outside = home.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, task.join("escape")).unwrap();
    symlink(home.join("missing.md"), task.join("dangling")).unwrap();
    for path in [
        task.join("out"),
        task.join("escape/note.md"),
        task.join("dangling"),
    ] {
        assert_denied_without_open(path.to_str().unwrap(), &task, &home);
        assert!(read_verified(path.to_str().unwrap(), &task, &home)
            .unwrap_err()
            .starts_with("local_file_denied:"));
    }
}

#[cfg(unix)]
#[test]
fn missing_directory_special_file_and_opener_failure_have_stable_errors() {
    let (_, home, task) = roots("local-file-file-kinds");
    let directory = task.join("directory");
    std::fs::create_dir_all(&directory).unwrap();
    for (path, prefix) in [
        (task.join("missing.md"), "local_file_not_found:"),
        (directory, "local_file_invalid:"),
    ] {
        let error = open_spy(path.to_str().unwrap(), &task, &home).unwrap_err();
        assert!(error.starts_with(prefix), "{error}");
        let error = read_verified(path.to_str().unwrap(), &task, &home).unwrap_err();
        assert!(error.starts_with(prefix), "{error}");
    }
    let error = open_spy("null", Path::new("/dev"), &home).unwrap_err();
    assert!(error.starts_with("local_file_invalid:"), "{error}");
    let document = task.join("failure.md");
    std::fs::write(&document, "ok").unwrap();
    let error = open_verified("failure.md", &task, &home, |_| Err("failed".into())).unwrap_err();
    assert!(error.starts_with("local_file_open_failed:"), "{error}");
}

#[test]
fn mock_ipc_rejects_untrusted_label_before_state_and_allows_main_and_editor_to_reach_guard() {
    let app = tauri::test::mock_builder()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![open_local_file, read_local_file])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let request = |cmd: &str, path: &str| tauri::webview::InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: serde_json::json!({ "id": 1, "path": path }).into(),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.into(),
    };
    for label in ["inspector", "main", "editor"] {
        let webview = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .unwrap();
        let assert_error = |cmd| {
            let result = tauri::test::get_ipc_response(&webview, request(cmd, "note.md"));
            let error = format!("{result:?}");
            let expected = if label == "inspector" {
                "local_file_denied:"
            } else {
                "local_file_io:"
            };
            assert!(error.contains(expected), "{label}: {error}");
            if label != "inspector" {
                assert!(
                    error.contains("DB가 초기화되지 않았습니다"),
                    "{label}: {error}"
                );
            }
        };
        for cmd in ["open_local_file", "read_local_file"] {
            assert_error(cmd);
        }
        if label == "main" {
            let newline = crate::testtmp::dir().join("local-file-newline\n.md");
            std::fs::write(&newline, "ok").unwrap();
            for cmd in ["open_local_file", "read_local_file"] {
                let result = tauri::test::get_ipc_response(
                    &webview,
                    request(cmd, newline.to_str().unwrap()),
                );
                assert!(format!("{result:?}").contains("local_file_invalid:"));
            }
        }
    }
}
