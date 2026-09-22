#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::workflow::artifacts::{
    AncestorArtifact, ArtifactStore, CapturePolicy, GitInputPolicy,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static COUNT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = temp_root::dir().join(format!(
            "workflow-inputs-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn tree(&self, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        for (relative, bytes) in files {
            let target = path.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
        }
        path
    }
    fn store(&self) -> ArtifactStore {
        ArtifactStore::open(self.0.join("store")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn policy() -> CapturePolicy {
    CapturePolicy {
        include_paths: vec!["src".into()],
        exclude_paths: vec![],
        write_paths: vec!["src".into()],
    }
}
fn ancestor(id: &str, index: u32, artifact_id: &str) -> AncestorArtifact {
    AncestorArtifact {
        node_id: id.into(),
        topo_order: index,
        artifact_id: artifact_id.into(),
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}

#[test]
fn composes_nonoverlapping_text_changes_and_materializes_exact_candidate() {
    let f = Fixture::new();
    let base = f.tree("base", &[("src/a", b"one\ntwo\nthree\nfour\nfive\n")]);
    let left = f.tree("left", &[("src/a", b"ONE\ntwo\nthree\nfour\nfive\n")]);
    let right = f.tree("right", &[("src/a", b"one\ntwo\nthree\nfour\nFIVE\n")]);
    let store = f.store();
    let a = store.capture(&base, &left, &policy()).unwrap();
    let b = store.capture(&base, &right, &policy()).unwrap();
    let branches = vec![
        ancestor("left", 1, &a.artifact_id),
        ancestor("right", 1, &b.artifact_id),
        ancestor("left", 1, &a.artifact_id),
    ];
    let merged = store
        .compose_input(&a.parent_input_hash, branches.clone())
        .unwrap();
    assert_eq!(merged.applied_ancestors, ["left", "right"]);
    assert_eq!(
        store
            .compose_input(&a.parent_input_hash, branches.into_iter().rev().collect())
            .unwrap(),
        merged
    );
    let destination = f.0.join("candidate");
    store
        .materialize_input(&merged.input_tree_hash, &destination)
        .unwrap();
    assert_eq!(
        fs::read(destination.join("src/a")).unwrap(),
        b"ONE\ntwo\nthree\nfour\nFIVE\n"
    );
    fs::write(destination.join("src/a"), b"worker mutation").unwrap();
    assert_eq!(
        store.load_input_tree(&merged.input_tree_hash).unwrap(),
        merged.manifest
    );
    assert_eq!(
        fs::read(base.join("src/a")).unwrap(),
        b"one\ntwo\nthree\nfour\nfive\n"
    );
}

#[test]
fn preserves_conflicting_text_inputs_without_publishing_a_candidate() {
    let f = Fixture::new();
    let base = f.tree("base", &[("src/a", b"before\n")]);
    let left = f.tree("left", &[("src/a", b"left\n")]);
    let right = f.tree("right", &[("src/a", b"right\n")]);
    let store = f.store();
    let a = store.capture(&base, &left, &policy()).unwrap();
    let b = store.capture(&base, &right, &policy()).unwrap();
    let error = store
        .compose_input(
            &a.parent_input_hash,
            vec![
                ancestor("a", 0, &a.artifact_id),
                ancestor("b", 0, &b.artifact_id),
            ],
        )
        .unwrap_err();
    assert!(error.to_string().contains("input_conflict"));
    let conflict = fs::read_dir(f.0.join("store/conflicts"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(conflict.join("base")).unwrap(), b"before\n");
    assert_eq!(fs::read(conflict.join("ours")).unwrap(), b"left\n");
    assert_eq!(fs::read(conflict.join("theirs")).unwrap(), b"right\n");
    assert_eq!(fs::read_dir(f.0.join("store/inputs")).unwrap().count(), 1);
    assert!(!fs::read_dir(f.0.join("store")).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".input-stage")));
}

#[test]
fn rejects_binary_changes_tampered_receipts_and_directory_delete_add_conflicts() {
    let f = Fixture::new();
    let base = f.tree("base", &[("src/a", b"a\0")]);
    let left = f.tree("left", &[("src/a", b"b\0")]);
    let right = f.tree("right", &[("src/a", b"c\0")]);
    let store = f.store();
    let a = store.capture(&base, &left, &policy()).unwrap();
    let b = store.capture(&base, &right, &policy()).unwrap();
    assert!(store
        .compose_input(
            &a.parent_input_hash,
            vec![
                ancestor("a", 0, &a.artifact_id),
                ancestor("b", 0, &b.artifact_id)
            ]
        )
        .unwrap_err()
        .to_string()
        .contains("binary"));
    let empty = f.tree("deleted", &[]);
    let added = f.tree("added", &[("src/a", b"a\0"), ("src/new", b"new")]);
    let deletion = store.capture(&base, &empty, &policy()).unwrap();
    let addition = store.capture(&base, &added, &policy()).unwrap();
    for branches in [
        vec![
            ancestor("a", 0, &deletion.artifact_id),
            ancestor("b", 0, &addition.artifact_id),
        ],
        vec![
            ancestor("b", 0, &deletion.artifact_id),
            ancestor("a", 0, &addition.artifact_id),
        ],
    ] {
        assert!(store
            .compose_input(&a.parent_input_hash, branches)
            .unwrap_err()
            .to_string()
            .contains("input_conflict"));
    }
    fs::write(
        f.0.join("store/trees")
            .join(&a.output_tree_hash)
            .join("files/src/a"),
        b"tamper",
    )
    .unwrap();
    assert!(store
        .compose_input(&a.parent_input_hash, vec![ancestor("a", 0, &a.artifact_id)])
        .is_err());
}

#[test]
fn exports_pinned_commit_excluding_credentials_without_touching_checkout() {
    let f = Fixture::new();
    let repo = f.tree(
        "repo",
        &[
            ("src/a", b"committed"),
            (".env", b"fixture-not-a-secret"),
            (".env.example", b"KEY="),
            ("cache/output", b"generated"),
        ],
    );
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("src/a"), b"dirty change").unwrap();
    fs::write(repo.join("untracked"), b"not an input").unwrap();
    let before_status = git(&repo, &["status", "--porcelain"]);
    let store = f.store();
    let receipt = store
        .export_git_input(
            &repo,
            &commit,
            &GitInputPolicy {
                exclude_paths: vec!["cache".into()],
            },
        )
        .unwrap();
    assert_eq!(receipt.excluded_paths, [".env", "cache/output"]);
    let destination = f.0.join("input");
    store
        .materialize_input(&receipt.input_tree_hash, &destination)
        .unwrap();
    assert_eq!(fs::read(destination.join("src/a")).unwrap(), b"committed");
    assert!(destination.join(".env.example").is_file());
    for excluded in [".env", "cache", ".git", "untracked"] {
        assert!(!destination.join(excluded).exists());
    }
    assert_eq!(git(&repo, &["status", "--porcelain"]), before_status);
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), commit);
    assert!(store
        .export_git_input(&repo, "HEAD", &GitInputPolicy::default())
        .is_err());
    assert!(store
        .export_git_input(
            &repo,
            &commit,
            &GitInputPolicy {
                exclude_paths: vec!["../escape".into()]
            }
        )
        .is_err());
}

#[test]
fn git_preflight_rejects_lfs_symlinks_and_gitlinks() {
    let f = Fixture::new();
    let repo = f.tree(
        "repo",
        &[(
            "src/lfs",
            b"version https://git-lfs.github.com/spec/v1\noid sha256:123\nsize 100\n",
        )],
    );
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "lfs"]);
    let store = f.store();
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    assert!(store
        .export_git_input(&repo, &commit, &GitInputPolicy::default())
        .unwrap_err()
        .to_string()
        .contains("LFS"));
    fs::remove_file(repo.join("src/lfs")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/tmp", repo.join("link")).unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "symlink"]);
        let linked = git(&repo, &["rev-parse", "HEAD"]);
        assert!(store
            .export_git_input(&repo, &linked, &GitInputPolicy::default())
            .unwrap_err()
            .to_string()
            .contains("symlink"));
        fs::remove_file(repo.join("link")).unwrap();
        git(&repo, &["add", "-A"]);
    }
    git(
        &repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},module"),
        ],
    );
    git(&repo, &["commit", "-qm", "gitlink"]);
    let linked = git(&repo, &["rev-parse", "HEAD"]);
    assert!(store
        .export_git_input(&repo, &linked, &GitInputPolicy::default())
        .unwrap_err()
        .to_string()
        .contains("submodule"));
}

#[test]
fn capture_rejects_named_credentials_before_publishing_and_export_rejects_partial_clone() {
    let f = Fixture::new();
    let input = f.tree("input", &[("src/.env", b"fixture-only")]);
    let output = f.tree("output", &[("src/.env", b"fixture-only")]);
    let store = f.store();
    assert!(store
        .capture(&input, &output, &policy())
        .unwrap_err()
        .to_string()
        .contains("credential"));
    assert_eq!(fs::read_dir(f.0.join("store/inputs")).unwrap().count(), 0);
    let repo = f.tree("repo", &[("src/a", b"local")]);
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["config", "remote.origin.promisor", "true"]);
    assert!(store
        .export_git_input(&repo, &commit, &GitInputPolicy::default())
        .unwrap_err()
        .to_string()
        .contains("partial-clone"));
}

#[test]
fn common_ancestor_is_applied_once_before_descendant_deltas() {
    let f = Fixture::new();
    let base = f.tree("base", &[("src/a", b"base\n")]);
    let shared_tree = f.tree("shared", &[("src/a", b"shared\n")]);
    let left_tree = f.tree("left", &[("src/a", b"shared\n"), ("src/left", b"left")]);
    let right_tree = f.tree("right", &[("src/a", b"shared\n"), ("src/right", b"right")]);
    let store = f.store();
    let shared = store.capture(&base, &shared_tree, &policy()).unwrap();
    let left = store.capture(&shared_tree, &left_tree, &policy()).unwrap();
    let right = store.capture(&shared_tree, &right_tree, &policy()).unwrap();
    let input = store
        .compose_input(
            &shared.parent_input_hash,
            vec![
                ancestor("right", 2, &right.artifact_id),
                ancestor("shared", 0, &shared.artifact_id),
                ancestor("left", 1, &left.artifact_id),
                ancestor("shared", 0, &shared.artifact_id),
            ],
        )
        .unwrap();
    assert_eq!(input.applied_ancestors, ["shared", "left", "right"]);
    let destination = f.0.join("combined");
    store
        .materialize_input(&input.input_tree_hash, &destination)
        .unwrap();
    assert_eq!(fs::read(destination.join("src/a")).unwrap(), b"shared\n");
    assert_eq!(fs::read(destination.join("src/left")).unwrap(), b"left");
    assert_eq!(fs::read(destination.join("src/right")).unwrap(), b"right");
    assert!(store
        .compose_input(
            &shared.parent_input_hash,
            vec![
                ancestor("shared", 0, &shared.artifact_id),
                ancestor("shared", 0, &left.artifact_id)
            ]
        )
        .is_err());
}

#[cfg(unix)]
#[test]
fn merges_text_and_executable_bit_and_respects_snapshot_size_limits() {
    use praxis_lib::workflow::artifacts::ArtifactLimits;
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let base = f.tree("base", &[("src/script", b"one\ntwo\nthree\nfour\nfive\n")]);
    let left = f.tree("left", &[("src/script", b"ONE\ntwo\nthree\nfour\nfive\n")]);
    let right = f.tree("right", &[("src/script", b"one\ntwo\nthree\nfour\nFIVE\n")]);
    fs::set_permissions(left.join("src/script"), fs::Permissions::from_mode(0o755)).unwrap();
    let store = f.store();
    let a = store.capture(&base, &left, &policy()).unwrap();
    let b = store.capture(&base, &right, &policy()).unwrap();
    let input = store
        .compose_input(
            &a.parent_input_hash,
            vec![
                ancestor("a", 0, &a.artifact_id),
                ancestor("b", 0, &b.artifact_id),
            ],
        )
        .unwrap();
    assert_eq!(
        input
            .manifest
            .entries
            .iter()
            .find(|e| e.path == "src/script")
            .unwrap()
            .state
            .mode,
        0o755
    );
    let bounded = ArtifactStore::open_with_limits(
        f.0.join("bounded"),
        ArtifactLimits {
            max_snapshot_bytes: 3,
            max_file_bytes: 3,
        },
    )
    .unwrap();
    let repo = f.tree("repo", &[("src/a", b"larger than limit")]);
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    assert!(bounded
        .export_git_input(&repo, &commit, &GitInputPolicy::default())
        .unwrap_err()
        .to_string()
        .contains("limit"));
    assert_eq!(fs::read_dir(f.0.join("bounded/inputs")).unwrap().count(), 0);
}

#[test]
fn concurrent_identical_input_and_artifact_publications_are_idempotent() {
    let f = Fixture::new();
    let content = vec![b'x'; 512 * 1024];
    let input = f.tree("input", &[("src/a", &content)]);
    let output = f.tree("output", &[("src/a", b"modified")]);
    let store = f.store();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let results = std::thread::scope(|scope| {
        (0..4)
            .map(|_| {
                let barrier = barrier.clone();
                let store = &store;
                let input = &input;
                let output = &output;
                scope.spawn(move || {
                    barrier.wait();
                    store.capture(input, output, &policy()).unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|receipt| receipt == &results[0]));
    assert_eq!(fs::read_dir(f.0.join("store/inputs")).unwrap().count(), 1);
    assert_eq!(
        fs::read_dir(f.0.join("store/artifacts")).unwrap().count(),
        1
    );
    assert!(store.load(&results[0].artifact_id).is_ok());
}
