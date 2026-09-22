#[path = "support/temp_root.rs"]
mod temp_root;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use praxis_lib::workflow::{
    artifacts::{
        ArtifactStore, CapturePolicy, Delta, DeltaEntry, DeltaOperation, EntryKind, EntryState,
        TreeManifest,
    },
    inputs::{merge_ancestors, AncestorDelta},
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp(label: &str) -> PathBuf {
    let path = temp_root::dir().join(format!(
        "praxis-workflow-artifacts-{label}-{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, bytes).unwrap();
}

fn policy() -> CapturePolicy {
    CapturePolicy {
        include_paths: vec!["src".into()],
        exclude_paths: vec![],
        write_paths: vec!["src".into()],
    }
}

fn hash(letter: char) -> String {
    letter.to_string().repeat(64)
}

fn file(letter: char, mode: u32) -> EntryState {
    EntryState {
        kind: EntryKind::File,
        mode,
        size: 1,
        hash: hash(letter),
    }
}

fn directory() -> EntryState {
    EntryState {
        kind: EntryKind::Directory,
        mode: 0o755,
        size: 0,
        hash: hash('d'),
    }
}

fn manifest(entries: Vec<(&str, EntryState)>) -> TreeManifest {
    TreeManifest::from_states(
        entries
            .into_iter()
            .map(|(path, state)| (path.to_owned(), state))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap()
}

fn delta(
    path: &str,
    operation: DeltaOperation,
    before: Option<EntryState>,
    after: Option<EntryState>,
) -> Delta {
    Delta {
        entries: vec![DeltaEntry {
            path: path.into(),
            operation,
            before,
            after,
        }],
    }
}

#[test]
fn captures_roundtrips_and_idempotently_publishes_immutable_full_trees() {
    let root = temp("roundtrip");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    write(&input, "src/main.txt", b"before");
    write(&output, "src/main.txt", b"after");
    let store = ArtifactStore::open(root.join("store")).unwrap();
    let artifact = store.capture(&input, &output, &policy()).unwrap();
    let repeated = store.capture(&input, &output, &policy()).unwrap();
    assert_eq!(artifact, repeated);
    let different_parent = root.join("different-parent");
    fs::create_dir_all(&different_parent).unwrap();
    write(&different_parent, "src/main.txt", b"another parent");
    let distinct_artifact = store
        .capture(&different_parent, &output, &policy())
        .unwrap();
    assert_eq!(
        distinct_artifact.output_tree_hash,
        artifact.output_tree_hash
    );
    assert_ne!(distinct_artifact.artifact_id, artifact.artifact_id);
    let (loaded, tree, delta) = store.load(&artifact.artifact_id).unwrap();
    assert_eq!(loaded, artifact);
    assert_eq!(tree.hash().unwrap(), artifact.output_tree_hash);
    assert_eq!(delta.hash().unwrap(), artifact.delta_hash);
    assert_eq!(fs::read(input.join("src/main.txt")).unwrap(), b"before");
    assert_eq!(fs::read(output.join("src/main.txt")).unwrap(), b"after");
    let materialized = root.join("materialized");
    store.materialize(&artifact, &materialized).unwrap();
    assert_eq!(
        fs::read(materialized.join("src/main.txt")).unwrap(),
        b"after"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_ne!(
            fs::metadata(
                root.join("store/trees")
                    .join(&artifact.output_tree_hash)
                    .join("files/src/main.txt")
            )
            .unwrap()
            .ino(),
            fs::metadata(materialized.join("src/main.txt"))
                .unwrap()
                .ino(),
            "materialization must copy, not hardlink"
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_changes_outside_contract_or_write_scope_before_publication() {
    let root = temp("scope");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    write(&input, "src/ok", b"one");
    write(&output, "src/ok", b"one");
    write(&output, "secret.txt", b"changed");
    let store = ArtifactStore::open(root.join("store")).unwrap();
    assert!(store
        .capture(&input, &output, &policy())
        .unwrap_err()
        .to_string()
        .contains("output contract"));
    fs::remove_file(output.join("secret.txt")).unwrap();
    write(&output, "src/other", b"changed");
    assert!(ArtifactStore::open(root.join("other"))
        .unwrap()
        .capture(
            &input,
            &output,
            &CapturePolicy {
                include_paths: vec!["src".into()],
                exclude_paths: vec![],
                write_paths: vec!["src/ok".into()]
            }
        )
        .unwrap_err()
        .to_string()
        .contains("write_paths"));
    assert!(root
        .join("store/artifacts")
        .read_dir()
        .unwrap()
        .next()
        .is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn rejects_unsafe_paths_and_symlink_snapshots() {
    let root = temp("unsafe");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    write(&input, "src/a", b"one");
    write(&output, "src/a", b"two");
    let store = ArtifactStore::open(root.join("store")).unwrap();
    assert!(store
        .capture(
            &input,
            &output,
            &CapturePolicy {
                include_paths: vec!["../src".into()],
                exclude_paths: vec![],
                write_paths: vec!["src".into()]
            }
        )
        .is_err());
    for unsafe_path in ["src//a", "src/./a", "src/../a", "src/a\0b", "src/\n"] {
        assert!(
            store
                .capture(
                    &input,
                    &output,
                    &CapturePolicy {
                        include_paths: vec![unsafe_path.into()],
                        exclude_paths: vec![],
                        write_paths: vec!["src".into()]
                    }
                )
                .is_err(),
            "accepted unsafe path {unsafe_path:?}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("src/a", output.join("link")).unwrap();
        assert!(store
            .capture(&input, &output, &policy())
            .unwrap_err()
            .to_string()
            .contains("symbolic links"));
        fs::remove_file(output.join("link")).unwrap();
        let escaped = root.join("escaped");
        fs::create_dir_all(&escaped).unwrap();
        write(&escaped, "a", b"outside");
        fs::remove_dir_all(output.join("src")).unwrap();
        std::os::unix::fs::symlink(&escaped, output.join("src")).unwrap();
        assert!(store
            .capture(&input, &output, &policy())
            .unwrap_err()
            .to_string()
            .contains("symbolic links"));
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_rejects_modified_tree_blob_contents() {
    let root = temp("tamper");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    write(&input, "src/a", b"one");
    write(&output, "src/a", b"two");
    let store = ArtifactStore::open(root.join("store")).unwrap();
    let artifact = store.capture(&input, &output, &policy()).unwrap();
    fs::write(
        root.join("store/trees")
            .join(&artifact.output_tree_hash)
            .join("files/src/a"),
        b"tampered",
    )
    .unwrap();
    assert!(store
        .load(&artifact.artifact_id)
        .unwrap_err()
        .to_string()
        .contains("does not match"));
    fs::write(
        root.join("store/artifacts")
            .join(&artifact.artifact_id)
            .join("delta.json"),
        b"{}",
    )
    .unwrap();
    assert!(store.load(&artifact.artifact_id).is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_rejects_modified_parent_tree_blob_contents() {
    let root = temp("parent-tamper");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    write(&input, "src/a", b"one");
    write(&output, "src/a", b"two");
    let store = ArtifactStore::open(root.join("store")).unwrap();
    let artifact = store.capture(&input, &output, &policy()).unwrap();
    fs::write(
        root.join("store/inputs")
            .join(&artifact.parent_input_hash)
            .join("files/src/a"),
        b"tampered",
    )
    .unwrap();
    assert!(store
        .load(&artifact.artifact_id)
        .unwrap_err()
        .to_string()
        .contains("does not match"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_paths_are_globally_bytewise_sorted() {
    let root = temp("ordering");
    let input = root.join("input");
    let output = root.join("output");
    fs::create_dir_all(&input).unwrap();
    fs::create_dir_all(&output).unwrap();
    for tree in [&input, &output] {
        write(tree, "src/foo/child", b"child");
        write(tree, "src/foo.rs", b"sibling");
    }
    let store = ArtifactStore::open(root.join("store")).unwrap();
    let artifact = store.capture(&input, &output, &policy()).unwrap();
    let tree = store.load_tree(&artifact.output_tree_hash).unwrap();
    assert_eq!(
        tree.entries
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>(),
        vec!["src", "src/foo", "src/foo.rs", "src/foo/child"]
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn merges_diamond_ancestor_once_in_stable_topological_order() {
    let base = manifest(vec![]);
    let shared = delta("shared", DeltaOperation::Add, None, Some(file('a', 0o644)));
    let result = merge_ancestors(
        &base,
        vec![
            AncestorDelta {
                node_id: "shared".into(),
                topo_order: 0,
                delta: shared.clone(),
            },
            AncestorDelta {
                node_id: "shared".into(),
                topo_order: 0,
                delta: shared,
            },
        ],
    )
    .unwrap();
    assert_eq!(result.applied_ancestors, vec!["shared"]);
    assert_eq!(result.manifest.entries.len(), 1);
    assert!(merge_ancestors(
        &base,
        vec![
            AncestorDelta {
                node_id: "shared".into(),
                topo_order: 0,
                delta: delta("shared", DeltaOperation::Add, None, Some(file('a', 0o644)))
            },
            AncestorDelta {
                node_id: "shared".into(),
                topo_order: 0,
                delta: delta("shared", DeltaOperation::Add, None, Some(file('b', 0o644)))
            },
        ]
    )
    .unwrap_err()
    .to_string()
    .contains("different content"));
}

#[test]
fn merge_preserves_deletions_and_rejects_delete_modify_and_type_conflicts() {
    let base = manifest(vec![("item", file('a', 0o644))]);
    let delete = delta("item", DeltaOperation::Delete, Some(file('a', 0o644)), None);
    let deleted = merge_ancestors(
        &base,
        vec![AncestorDelta {
            node_id: "delete".into(),
            topo_order: 1,
            delta: delete.clone(),
        }],
    )
    .unwrap();
    assert!(deleted.manifest.entries.is_empty());
    let modify = delta(
        "item",
        DeltaOperation::Modify,
        Some(file('a', 0o644)),
        Some(file('b', 0o644)),
    );
    assert!(merge_ancestors(
        &base,
        vec![
            AncestorDelta {
                node_id: "modify".into(),
                topo_order: 1,
                delta: modify
            },
            AncestorDelta {
                node_id: "delete".into(),
                topo_order: 2,
                delta: delete
            },
        ]
    )
    .unwrap_err()
    .to_string()
    .contains("input_conflict"));
    let replace = delta(
        "item",
        DeltaOperation::ReplaceType,
        Some(file('a', 0o644)),
        Some(directory()),
    );
    let binary = delta(
        "item",
        DeltaOperation::Modify,
        Some(file('a', 0o644)),
        Some(file('c', 0o644)),
    );
    assert!(merge_ancestors(
        &base,
        vec![
            AncestorDelta {
                node_id: "type".into(),
                topo_order: 1,
                delta: replace
            },
            AncestorDelta {
                node_id: "binary".into(),
                topo_order: 2,
                delta: binary
            },
        ]
    )
    .is_err());
    let binary_left = delta(
        "item",
        DeltaOperation::Modify,
        Some(file('a', 0o644)),
        Some(file('b', 0o644)),
    );
    let binary_right = delta(
        "item",
        DeltaOperation::Modify,
        Some(file('a', 0o644)),
        Some(file('c', 0o644)),
    );
    assert!(merge_ancestors(
        &base,
        vec![
            AncestorDelta {
                node_id: "binary-left".into(),
                topo_order: 1,
                delta: binary_left
            },
            AncestorDelta {
                node_id: "binary-right".into(),
                topo_order: 2,
                delta: binary_right
            },
        ]
    )
    .unwrap_err()
    .to_string()
    .contains("input_conflict"));
}

#[test]
fn merge_combines_one_mode_change_with_the_other_branchs_content_change() {
    let base = manifest(vec![("bin", file('a', 0o644))]);
    let mode = delta(
        "bin",
        DeltaOperation::Modify,
        Some(file('a', 0o644)),
        Some(file('a', 0o755)),
    );
    let content = delta(
        "bin",
        DeltaOperation::Modify,
        Some(file('a', 0o644)),
        Some(file('b', 0o644)),
    );
    let result = merge_ancestors(
        &base,
        vec![
            AncestorDelta {
                node_id: "content".into(),
                topo_order: 1,
                delta: content,
            },
            AncestorDelta {
                node_id: "mode".into(),
                topo_order: 2,
                delta: mode,
            },
        ],
    )
    .unwrap();
    assert_eq!(result.manifest.entries[0].state, file('b', 0o755));
}
