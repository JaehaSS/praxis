use super::*;
use std::ffi::CString;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn fixture(name: &str) -> std::path::PathBuf {
    let path = crate::testtmp::dir().join(format!(
        "scoped-file-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn rejects_symlink_roots_parents_leaves_and_fifo() {
    let root = fixture("unsafe");
    let outside = fixture("outside");
    fs::write(root.join("regular.md"), "ok").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("leaf.md")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("middle")).unwrap();
    let linked_root = root.parent().unwrap().join("scoped-file-linked-root");
    std::os::unix::fs::symlink(&root, &linked_root).unwrap();
    let fifo = root.join("pipe.md");
    let name = CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { nix::libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(open(&linked_root, "regular.md").is_err());
    assert!(open(&root, "middle/file.md").is_err());
    assert!(open(&root, "leaf.md").is_err());
    assert!(!open(&root, "pipe.md")
        .unwrap()
        .metadata()
        .unwrap()
        .is_file());
    assert!(list(&linked_root, ".", 1).is_err());
    assert!(list(&root, "middle", 1).is_err());
}

#[test]
fn enumerates_a_bounded_directory_from_its_open_descriptor() {
    let root = fixture("listing");
    for index in 0..201 {
        fs::write(root.join(format!("{index}.md")), "ok").unwrap();
    }
    let entries = list(&root, ".", 200).unwrap();
    assert_eq!(entries.names.len(), 200);
    assert_eq!(entries.inspected_entries, 201);
    assert!(entries.limited);
}

#[test]
fn checks_the_opened_root_identity() {
    use std::os::unix::fs::MetadataExt;

    let root = fixture("identity");
    fs::write(root.join("regular.md"), "ok").unwrap();
    let metadata = fs::metadata(&root).unwrap();
    assert!(open_verified(
        &root,
        "regular.md",
        metadata.dev() as i64,
        metadata.ino() as i64
    )
    .is_ok());
    assert!(open_verified(
        &root,
        "regular.md",
        metadata.dev() as i64,
        metadata.ino() as i64 + 1
    )
    .is_err());
}
