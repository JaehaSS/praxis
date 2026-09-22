use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn fixture(name: &str) -> PathBuf {
    let root = crate::testtmp::dir().join(format!(
        "experience-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write(root: &Path, relative: &str, text: impl AsRef<[u8]>) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn project_owner(root: &Path) -> DocumentOwner {
    DocumentOwner::Project {
        project: ProjectRef {
            host: Host::Local,
            project_key: root.to_string_lossy().to_string(),
        },
    }
}

#[test]
fn lists_each_source_and_project_references_once() {
    let project = fixture("project");
    let home = fixture("home");
    write(
        &project,
        ".claude/skills/workflow-harness/SKILL.md",
        "skill",
    );
    write(
        &project,
        ".claude/skills/workflow-harness/LESSONS_LEARNED.md",
        "lesson",
    );
    write(&project, "LESSONS_LEARNED.md", "project lesson");
    write(&project, "docs/lessons-seed.md", "seed");
    write(&project, "docs/memory/memory.md", "memory");
    write(&project, "docs/archive/archive.md", "archive");
    let ListResult::Ready {
        sources,
        project: listing,
        ..
    } = list(&project, &home, HarnessName::WorkflowHarness)
    else {
        panic!("expected ready listing")
    };
    assert_eq!(sources.len(), 4);
    assert!(matches!(sources[0], SourceListing::Ready { .. }));
    assert!(matches!(sources[1], SourceListing::NotInstalled { .. }));
    let ProjectListing::Ready { documents, .. } = listing else {
        panic!("expected project references")
    };
    let keys: Vec<_> = documents.into_iter().map(|document| document.key).collect();
    assert_eq!(
        keys,
        [
            "archive/archive.md",
            "lessons-seed",
            "memory/memory.md",
            "project-lessons"
        ]
    );
}

#[test]
fn validates_document_owners_keys_and_content_errors() {
    let project = fixture("read");
    let home = fixture("read-home");
    write(&project, "docs/memory/valid.md", "valid");
    let ready = read(&project, &home, project_owner(&project), "memory/valid.md");
    assert!(matches!(ready, ReadResult::Ready { .. }));
    let invalid = read(
        &project,
        &home,
        project_owner(&project),
        "memory/../valid.md",
    );
    assert!(matches!(
        invalid,
        ReadResult::Error {
            error: ReadErrorOrRequest::Request(RequestError::InvalidRequest)
        }
    ));
    write(&project, "docs/memory/binary.md", [0xff]);
    let invalid_utf8 = read(&project, &home, project_owner(&project), "memory/binary.md");
    assert!(matches!(
        invalid_utf8,
        ReadResult::Error {
            error: ReadErrorOrRequest::Read(ReadError::InvalidUtf8)
        }
    ));
    write(&project, "docs/memory/large.md", vec![b'x'; 128 * 1024 + 1]);
    let oversized = read(&project, &home, project_owner(&project), "memory/large.md");
    assert!(matches!(
        oversized,
        ReadResult::Error {
            error: ReadErrorOrRequest::Read(ReadError::Oversized)
        }
    ));
}

#[test]
fn classifies_generated_project_lessons_only_with_the_fixed_source_set() {
    let project = fixture("generated");
    let home = fixture("generated-home");
    write(&project, "docs/memory/item.md", "memory");
    write(&project, "docs/archive/item.md", "archive");
    write(&project, "docs/lessons-seed.md", "seed");
    write(
        &project,
        "LESSONS_LEARNED.md",
        "<!-- 생성물: npm run docs:project — 손으로 고치지 마시오 -->\n# Praxis — 교훈 (전 기간)\n",
    );
    let generated = read(&project, &home, project_owner(&project), "project-lessons");
    assert!(matches!(
        generated,
        ReadResult::Ready {
            document: ExperienceDocument {
                generated: true,
                source_resolution: SourceResolution::KnownSet,
                ..
            }
        }
    ));
    fs::remove_file(project.join("docs/lessons-seed.md")).unwrap();
    let fallback = read(&project, &home, project_owner(&project), "project-lessons");
    assert!(matches!(
        fallback,
        ReadResult::Ready {
            document: ExperienceDocument {
                generated: false,
                source_resolution: SourceResolution::Unconfirmed,
                ..
            }
        }
    ));
}

#[test]
fn reports_mutation_after_the_open_handle_read() {
    let project = fixture("changed");
    write(&project, "document.md", "first");
    let path = project.join("document.md");
    let result = read::read_relative(&project, "document.md", || {
        fs::write(path, "changed").unwrap()
    });
    assert_eq!(result.unwrap_err(), ReadError::Changed);
}

#[test]
#[cfg(unix)]
fn registered_roots_require_canonical_exact_equality() {
    let root = fixture("registered");
    let alias = root.parent().unwrap().join(format!(
        "{}-alias",
        root.file_name().unwrap().to_string_lossy()
    ));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    let known = vec![root.to_string_lossy().to_string()];
    assert_eq!(
        registered_root(&alias.to_string_lossy(), &known),
        root.canonicalize().ok()
    );
    let sibling = fixture("sibling");
    let child = root.join("child");
    fs::create_dir(&child).unwrap();
    assert!(registered_root(&sibling.to_string_lossy(), &known).is_none());
    assert!(registered_root(&child.to_string_lossy(), &known).is_none());
    assert!(registered_root("/definitely-not-registered", &known).is_none());
    let deleted = fixture("deleted");
    let deleted_known = vec![deleted.to_string_lossy().to_string()];
    fs::remove_dir(&deleted).unwrap();
    assert!(registered_root(&root.to_string_lossy(), &deleted_known).is_none());
}
