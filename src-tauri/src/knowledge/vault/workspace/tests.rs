#![cfg(unix)]
use super::*;

fn root() -> VaultRoot {
    let path =
        crate::testtmp::dir().join(super::super::catalog::identifier("wiki-workspace").unwrap());
    std::fs::create_dir_all(&path).unwrap();
    let identity = super::super::platform::verified_root(&path).unwrap();
    VaultRoot {
        path: PathBuf::from(identity.canonical_root),
        device: identity.device,
        inode: identity.inode,
    }
}

#[test]
fn create_update_conflict_and_duplicate_preserve_the_file() {
    let root = root();
    storage::save(&root, "문서.md", "# 첫 문서\n", None).unwrap();
    let original = read_at(&root, "문서.md").unwrap();
    assert!(storage::save(&root, "문서.md", "덮어쓰기", None).is_err());
    storage::save(&root, "문서.md", "# 수정\n", Some(&original.sha256)).unwrap();
    assert!(storage::save(&root, "문서.md", "오래된 편집", Some(&original.sha256)).is_err());
    assert_eq!(read_at(&root, "문서.md").unwrap().content, "# 수정\n");
    assert_eq!(std::fs::read_dir(&root.path).unwrap().count(), 1);
    std::fs::remove_dir_all(root.path).unwrap();
}

#[test]
fn paths_and_symlinks_cannot_escape_or_modify_instructions() {
    for path in [
        "../x.md",
        "/x.md",
        "a/../x.md",
        "a\\x.md",
        ".git/x.md",
        "AGENTS.md",
        "a/claude.md",
        "raw/x.md",
        "a//x.md",
        "a.txt",
        "wiki/index.md",
    ] {
        assert!(page_path(path).is_err(), "{path}");
    }
    let root = root();
    let outside = crate::testtmp::dir().join(super::super::catalog::identifier("outside").unwrap());
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.md"), "private").unwrap();
    std::os::unix::fs::symlink(&outside, root.path.join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.md"), root.path.join("leaf.md")).unwrap();
    assert!(read_at(&root, "linked/secret.md").is_err());
    assert!(storage::save(&root, "linked/new.md", "bad", None).is_err());
    assert!(read_at(&root, "leaf.md").is_err());
    assert!(storage::save(&root, "leaf.md", "bad", None).is_err());
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.md")).unwrap(),
        "private"
    );
    std::fs::remove_dir_all(root.path).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}

#[test]
fn trash_checks_conflict_and_keeps_file_on_os_failure() {
    let root = root();
    storage::save(&root, "a.md", "original", None).unwrap();
    let doc = read_at(&root, "a.md").unwrap();
    assert!(storage::trash_with(&root, "a.md", "stale", |_| panic!("must not trash")).is_err());
    assert!(
        storage::trash_with(&root, "a.md", &doc.sha256, |_| anyhow::bail!(
            "OS trash unavailable"
        ))
        .is_err()
    );
    assert_eq!(read_at(&root, "a.md").unwrap().content, "original");
    // Inject a reversible move: no real user trash is touched by tests.
    storage::trash_with(&root, "a.md", &doc.sha256, |path| {
        std::fs::rename(path, root.path.join("recovered.txt"))?;
        Ok(())
    })
    .unwrap();
    assert!(!root.path.join("a.md").exists());
    assert_eq!(
        std::fs::read_to_string(root.path.join("recovered.txt")).unwrap(),
        "original"
    );
    std::fs::remove_dir_all(root.path).unwrap();
}

#[tokio::test]
async fn inactive_and_read_only_vaults_reject_mutations() {
    let pool = crate::knowledge::tests::raw_pool().await;
    super::super::migrate(&pool).await.unwrap();
    let root = root();
    let vault = super::super::register_vault(&pool, &root.path, 1)
        .await
        .unwrap();
    save(&pool, &vault.id, "a.md", "original", None)
        .await
        .unwrap();
    sqlx::query("UPDATE vaults SET writable = 0 WHERE id = ?")
        .bind(&vault.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(save(&pool, &vault.id, "a.md", "bad", None).await.is_err());
    assert!(trash(&pool, &vault.id, "a.md", &hash(b"original"))
        .await
        .is_err());
    sqlx::query("UPDATE vaults SET enabled = 0 WHERE id = ?")
        .bind(&vault.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(read(&pool, &vault.id, "a.md").await.is_err());
    std::fs::remove_dir_all(root.path).unwrap();
}

#[test]
fn stale_and_invalid_graphs_are_rejected() {
    let root = root();
    storage::save(&root, "a.md", "# A", None).unwrap();
    let payload = serde_json::json!({"schema_version":1,"nodes":[{"id":"a.md","path":"a.md","title":"A","aliases":[],"tags":[],"body":"# A","source_prefix":"","outgoing":[],"backlinks":[]}],"edges":[],"diagnostics":[]});
    let mut graph: Graph = serde_json::from_value(payload).unwrap();
    validate_graph(&root, &mut graph).unwrap();
    assert_eq!(graph.nodes[0].sha256, hash(b"# A"));
    // Front-matter classification is optional; a document without it must still load.
    assert_eq!(graph.nodes[0].kind, "");
    graph.nodes[0].body = "stale".into();
    assert!(validate_graph(&root, &mut graph).is_err());
    graph.schema_version = 2;
    assert!(validate_graph(&root, &mut graph).is_err());
    std::fs::remove_dir_all(root.path).unwrap();
}

#[test]
fn front_matter_classification_reaches_the_frontend_under_its_own_names() {
    let payload = serde_json::json!({"id":"a.md","path":"a.md","title":"A","aliases":[],"tags":[],
        "type":"decision","status":"active","scope":"team","body":"# A","source_prefix":"","outgoing":[],"backlinks":[]});
    let page: Page = serde_json::from_value(payload).unwrap();

    assert_eq!((page.kind.as_str(), page.status.as_str(), page.scope.as_str()), ("decision", "active", "team"));

    let encoded = serde_json::to_value(&page).unwrap();

    assert_eq!(encoded["type"], "decision");
    assert!(encoded.get("kind").is_none());
}

#[tokio::test]
#[ignore = "requires locally installed knowledge-harness and Python 3.10+"]
async fn installed_parser_round_trip() {
    let root = root();
    storage::save(&root, "a.md", "# A\n[문서 B](b.md)\n", None).unwrap();
    storage::save(&root, "b.md", "---\naliases: [별칭]\n---\n# B\n", None).unwrap();
    let scripts = parser_scripts(Path::new(&std::env::var("HOME").unwrap())).unwrap();
    let mut graph = run_parser(&root.path, &scripts).await.unwrap();
    validate_graph(&root, &mut graph).unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].target, "b.md");
    std::fs::remove_file(root.path.join("b.md")).unwrap();
    let graph = run_parser(&root.path, &scripts).await.unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.edges.is_empty());
    assert_eq!(graph.diagnostics[0].kind, "broken_link");
    std::fs::remove_dir_all(root.path).unwrap();
}

#[test]
fn trash_recovery_never_overwrites_a_recreated_file() {
    let root = root();
    storage::save(&root, "a.md", "original", None).unwrap();
    let doc = read_at(&root, "a.md").unwrap();
    let preserved = std::cell::RefCell::new(PathBuf::new());
    let error = storage::trash_with(&root, "a.md", &doc.sha256, |path| {
        *preserved.borrow_mut() = path.to_owned();
        std::fs::write(root.path.join("a.md"), "concurrent new document")?;
        anyhow::bail!("OS failure")
    })
    .unwrap_err();
    assert!(error.to_string().contains("파일을 보존했습니다"));
    assert_eq!(
        read_at(&root, "a.md").unwrap().content,
        "concurrent new document"
    );
    let staged = preserved.into_inner();
    assert_eq!(std::fs::read_to_string(&staged).unwrap(), "original");
    std::fs::remove_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(root.path).unwrap();
}
