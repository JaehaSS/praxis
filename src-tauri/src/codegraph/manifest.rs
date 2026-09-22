//! 소스 manifest와 빌드 전후 fingerprint.
//!
//! 어떤 확장자를 담는지는 여기서 정하지 않는다 — `lspclient::server::spec_for_path`가 유일한
//! 원천이다(설계 0065 DR-1). 새 언어는 `server.rs` 한 곳에서 늘어난다.
//! 수집 범위는 `.gitignore`와 바닥 목록이 정한다(DR-5).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::lspclient::server::{self, ServerSpec};

/// 파일 상한. `reference_build::link`가 심볼당 `references` 1회를 쓰므로 언어가 늘면 요청이
/// 곱으로 는다(설계 0065 DR-5).
///
/// code Wiki의 `MAX_FILES`(5,000)보다 **높아야 한다.** 같거나 낮으면 manifest가 먼저 잘라
/// Wiki의 초과 거부가 영원히 발화하지 않는다 — 자르는 것과 거부하는 것은 다른 정책이다.
pub const MAX_FILES: usize = 20_000;

/// `.gitignore`가 적어 두지 않아도 항상 제외한다 — 실제로 적지 않은 저장소가 있다.
const ALWAYS_SKIP: [&str; 7] = [
    ".git",
    ".praxis",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub content_hash: String,
    /// LSP `languageId` — `"python"`·`"typescriptreact"` 등. **서버 키가 아니다**(DR-4).
    pub lang: &'static str,
    /// 이 파일을 맡는 `ServerSpec::key`. build가 이것으로 파일을 서버별로 묶는다.
    pub spec_key: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceManifest {
    pub files: Vec<ManifestFile>,
    /// 상한 적용 **전에** 수집된 소스 수. `files.len()`보다 크면 잘렸다는 뜻이다.
    pub files_total: usize,
    pub fingerprint: String,
}

/// fingerprint는 **남긴 파일과 그 언어들의 설정 파일**로만 계산한다 — 그래프에 실제로 들어간
/// 입력과 일치해야 같은 트리가 항상 같은 값을 내고, 폴링마다 `stale`로 뒤집히지 않는다.
pub fn scan(root: &Path) -> anyhow::Result<SourceManifest> {
    let (sources, others) = walk(root)?;
    let files_total = sources.len();
    let sources = retain(sources, MAX_FILES);
    let mut configuration = configuration_files(&sources, others);
    configuration.sort();

    let mut files = hash(sources)?;
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let fingerprint = fingerprint(root, &files, &configuration)?;
    Ok(SourceManifest {
        files,
        files_total,
        fingerprint,
    })
}

struct Candidate {
    rel_path: String,
    abs_path: PathBuf,
    spec: ServerSpec,
    lang: &'static str,
}

fn walk(root: &Path) -> anyhow::Result<(Vec<Candidate>, Vec<PathBuf>)> {
    let mut sources = Vec::new();
    let mut others = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .git_global(false)
        // git 저장소가 아닌 디렉터리에서도 `.gitignore`를 존중한다(`fsapi/search.rs`와 같은 조합).
        .require_git(false)
        .parents(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !ALWAYS_SKIP.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .build();
    for entry in walker.flatten() {
        // 심볼릭 링크는 따라가지도 담지도 않는다 — 워크트리 밖 소스가 섞이면 안 된다.
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.into_path();
        let Some((spec, lang)) = server::spec_for_path(&path) else {
            others.push(path);
            continue;
        };
        let rel_path = path.strip_prefix(root)?.to_string_lossy().into_owned();
        sources.push(Candidate {
            rel_path,
            abs_path: path,
            spec,
            lang,
        });
    }
    Ok((sources, others))
}

/// 상한을 넘으면 **루트에서 가까운 순**(깊이 오름차순, 동률은 사전순)으로 남긴다.
/// 사전순으로 자르면 `.venv/…`가 `src/…`보다 앞서 사용자 소스가 먼저 잘린다(DR-5).
fn retain(mut sources: Vec<Candidate>, limit: usize) -> Vec<Candidate> {
    if sources.len() <= limit {
        return sources;
    }
    sources.sort_by(|a, b| {
        depth(&a.rel_path)
            .cmp(&depth(&b.rel_path))
            .then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    sources.truncate(limit);
    sources
}

fn depth(rel_path: &str) -> usize {
    Path::new(rel_path).components().count()
}

/// 수집한 소스가 실제로 쓰는 서버들의 `config_files` 합집합만 fingerprint 입력으로 삼는다.
/// 목록을 여기에 다시 적으면 서버를 더할 때 한쪽만 고쳐진다(DR-1).
fn configuration_files(sources: &[Candidate], others: Vec<PathBuf>) -> Vec<PathBuf> {
    let names: BTreeSet<&'static str> = sources
        .iter()
        .flat_map(|source| source.spec.config_files.iter().copied())
        .collect();
    others
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| names.contains(name))
        })
        .collect()
}

fn hash(sources: Vec<Candidate>) -> anyhow::Result<Vec<ManifestFile>> {
    let mut files = Vec::with_capacity(sources.len());
    for source in sources {
        let bytes = std::fs::read(&source.abs_path)?;
        files.push(ManifestFile {
            rel_path: source.rel_path,
            abs_path: source.abs_path,
            content_hash: format!("{:x}", Sha256::digest(bytes)),
            lang: source.lang,
            spec_key: source.spec.key,
        });
    }
    Ok(files)
}

fn fingerprint(
    root: &Path,
    files: &[ManifestFile],
    configuration: &[PathBuf],
) -> anyhow::Result<String> {
    let mut digest = Sha256::new();
    for file in files {
        digest.update(file.rel_path.as_bytes());
        digest.update([0]);
        digest.update(file.content_hash.as_bytes());
        digest.update([b'\n']);
    }
    for path in configuration {
        let relative = path.strip_prefix(root)?.to_string_lossy();
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(format!("{:x}", Sha256::digest(std::fs::read(path)?)).as_bytes());
        digest.update([b'\n']);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn workspace() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            crate::testtmp::dir().join(format!("codegraph-manifest-{}-{n}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        root
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn paths(manifest: &SourceManifest) -> Vec<&str> {
        manifest
            .files
            .iter()
            .map(|file| file.rel_path.as_str())
            .collect()
    }

    fn candidate(rel_path: &str) -> Candidate {
        let (spec, lang) = server::spec_for_path(Path::new(rel_path)).unwrap();
        Candidate {
            rel_path: rel_path.to_string(),
            abs_path: PathBuf::from(rel_path),
            spec,
            lang,
        }
    }

    #[test]
    fn fingerprint_is_stable_and_ignores_target() {
        let root = workspace();
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\n").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/generated.rs"), "fn ignored() {}\n").unwrap();

        let first = scan(&root).unwrap();
        let second = scan(&root).unwrap();

        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.files.len(), 1);
        assert_eq!(first.files[0].rel_path, "src/lib.rs");
    }

    #[test]
    fn source_change_changes_the_fingerprint() {
        let root = workspace();
        let path = root.join("src/lib.rs");
        std::fs::write(&path, "fn a() {}\n").unwrap();
        let before = scan(&root).unwrap();
        std::fs::write(&path, "fn b() {}\n").unwrap();
        let after = scan(&root).unwrap();

        assert_ne!(before.fingerprint, after.fingerprint);
    }

    #[test]
    fn cargo_configuration_changes_the_fingerprint_without_entering_files() {
        let root = workspace();
        std::fs::write(root.join("src/lib.rs"), "fn a() {}\n").unwrap();
        let before = scan(&root).unwrap();
        std::fs::create_dir_all(root.join("src-tauri")).unwrap();
        std::fs::write(
            root.join("src-tauri/Cargo.toml"),
            "[package]\nname='nested'\n",
        )
        .unwrap();
        let after = scan(&root).unwrap();

        assert_ne!(before.fingerprint, after.fingerprint);
        assert_eq!(after.files.len(), 1);
    }

    #[test]
    fn python_configuration_changes_the_fingerprint() {
        let root = workspace();
        write(&root, "app.py", "def a():\n    pass\n");
        write(&root, "pyproject.toml", "[project]\nname='fixture'\n");
        let before = scan(&root).unwrap();
        let unchanged = scan(&root).unwrap();
        write(&root, "pyproject.toml", "[project]\nname='changed'\n");
        let after = scan(&root).unwrap();

        assert_eq!(before.fingerprint, unchanged.fingerprint);
        assert_ne!(before.fingerprint, after.fingerprint);
    }

    #[test]
    fn every_supported_language_is_collected() {
        let root = workspace();
        write(&root, "src/lib.rs", "fn a() {}\n");
        write(&root, "api/app.py", "def a():\n    pass\n");
        write(&root, "src/Main.java", "class Main {}\n");
        write(&root, "src/App.tsx", "export const A = () => null;\n");

        let manifest = scan(&root).unwrap();

        assert_eq!(
            paths(&manifest),
            ["api/app.py", "src/App.tsx", "src/Main.java", "src/lib.rs"]
        );
        assert_eq!(manifest.files_total, 4);
    }

    #[test]
    fn tsx_carries_the_language_id_not_the_server_key() {
        let root = workspace();
        write(&root, "src/App.tsx", "export const A = () => null;\n");

        let manifest = scan(&root).unwrap();
        let file = &manifest.files[0];

        assert_eq!(file.lang, "typescriptreact");
        assert_eq!(file.spec_key, "typescript-language-server");
        assert_ne!(file.lang, file.spec_key);
    }

    #[test]
    fn dependency_directories_are_skipped_without_a_gitignore() {
        let root = workspace();
        write(&root, "src/lib.rs", "fn a() {}\n");
        write(&root, ".venv/lib/site-packages/dep.py", "x = 1\n");
        write(&root, "src/__pycache__/cached.py", "x = 1\n");
        write(&root, "node_modules/pkg/index.js", "module.exports = 1;\n");
        write(&root, "dist/bundle.js", "1;\n");

        assert!(!root.join(".gitignore").exists());
        assert_eq!(paths(&scan(&root).unwrap()), ["src/lib.rs"]);
    }

    #[test]
    fn gitignored_paths_are_skipped() {
        let root = workspace();
        write(&root, ".gitignore", "generated/\nsecret.py\n");
        write(&root, "src/lib.rs", "fn a() {}\n");
        write(&root, "generated/model.py", "x = 1\n");
        write(&root, "secret.py", "x = 1\n");

        assert_eq!(paths(&scan(&root).unwrap()), ["src/lib.rs"]);
    }

    #[test]
    fn deleting_a_source_removes_it_from_the_next_snapshot_manifest() {
        let root = workspace();
        let kept = root.join("src/kept.rs");
        let deleted = root.join("src/deleted.rs");
        std::fs::write(&kept, "fn kept() {}\n").unwrap();
        std::fs::write(&deleted, "fn deleted() {}\n").unwrap();
        let before = scan(&root).unwrap();

        std::fs::remove_file(deleted).unwrap();
        let after = scan(&root).unwrap();

        assert_eq!(before.files.len(), 2);
        assert_eq!(after.files.len(), 1);
        assert_eq!(after.files[0].rel_path, "src/kept.rs");
        assert_ne!(before.fingerprint, after.fingerprint);
    }

    #[test]
    fn truncation_keeps_shallow_sources_and_drops_deep_ones() {
        let sources = vec![
            candidate("vendor/lib/python3/site-packages/pkg/mod.py"),
            candidate("src/lib.rs"),
            candidate("vendor/lib/dep.py"),
            candidate("app.py"),
        ];

        let kept: Vec<String> = retain(sources, 2)
            .into_iter()
            .map(|source| source.rel_path)
            .collect();

        assert_eq!(kept, ["app.py", "src/lib.rs"]);
    }

    #[test]
    fn truncation_leaves_a_short_manifest_untouched() {
        let kept = retain(vec![candidate("src/lib.rs")], MAX_FILES);
        assert_eq!(kept.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_sources_are_not_followed_outside_the_worktree() {
        use std::os::unix::fs::symlink;

        let root = workspace();
        let outside = crate::testtmp::dir().join("codegraph-outside.rs");
        std::fs::write(&outside, "fn outside() {}\n").unwrap();
        symlink(&outside, root.join("src/outside.rs")).unwrap();

        assert!(scan(&root).unwrap().files.is_empty());
    }
}
