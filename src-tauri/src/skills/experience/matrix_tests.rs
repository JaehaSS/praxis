use super::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(super) fn fixture(name: &str) -> PathBuf {
    let root = crate::testtmp::dir().join(format!(
        "experience-matrix-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

pub(super) fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

pub(super) fn owner(root: &Path) -> DocumentOwner {
    DocumentOwner::Project {
        project: ProjectRef {
            host: Host::Local,
            project_key: root.to_string_lossy().to_string(),
        },
    }
}

fn source(harness: HarnessName, vendor: Vendor, scope: Scope) -> SourceRef {
    SourceRef {
        host: Host::Local,
        harness,
        vendor,
        scope,
    }
}

#[test]
fn reads_every_vendor_scope_and_harness_source_with_the_wire_contract() {
    let project = fixture("sources");
    let home = fixture("sources-home");
    for harness in [HarnessName::WorkflowHarness, HarnessName::LoopEngineering] {
        for vendor in [Vendor::Claude, Vendor::Codex] {
            for scope in [Scope::Project, Scope::Global] {
                let source = source(harness.clone(), vendor.clone(), scope.clone());
                let root = if scope == Scope::Project {
                    &project
                } else {
                    &home
                };
                let skill = discovery::skill_relative(&source);
                write(root, &skill, "skill");
                let lesson = format!("{}/LESSONS_LEARNED.md", skill.trim_end_matches("/SKILL.md"));
                write(
                    root,
                    &lesson,
                    &format!("{:?}-{:?}-{:?}", harness, vendor, scope),
                );
            }
        }
        let ListResult::Ready { sources, .. } = list(&project, &home, harness.clone()) else {
            panic!("ready")
        };
        assert_eq!(sources.len(), 4);
        for source in [
            source(harness.clone(), Vendor::Claude, Scope::Project),
            source(harness.clone(), Vendor::Claude, Scope::Global),
            source(harness.clone(), Vendor::Codex, Scope::Project),
            source(harness.clone(), Vendor::Codex, Scope::Global),
        ] {
            assert!(sources.iter().any(|listing| matches!(listing, SourceListing::Ready { source: found, .. } if found == &source)));
            let ReadResult::Ready { document } = read(
                &project,
                &home,
                DocumentOwner::Skill {
                    source: source.clone(),
                },
                "skill-lessons",
            ) else {
                panic!("source read")
            };
            let expected = format!(
                "{:?}-{:?}-{:?}",
                source.harness, source.vendor, source.scope
            );
            assert_eq!(document.text, expected);
            assert_eq!(
                document.content_hash,
                format!("{:x}", Sha256::digest(expected.as_bytes()))
            );
        }
    }
    let json = serde_json::to_value(ListResult::Ready {
        observed_at: "now".into(),
        sources: vec![SourceListing::Ready {
            source: source(HarnessName::WorkflowHarness, Vendor::Claude, Scope::Project),
            documents: vec![],
            limited: false,
            inspected_entries: 1,
        }],
        project: ProjectListing::Ready {
            project: ProjectRef {
                host: Host::Local,
                project_key: "root".into(),
            },
            documents: vec![],
            limited: false,
            inspected_entries: 1,
        },
    })
    .unwrap();
    assert_eq!(json["observedAt"], "now");
    assert_eq!(json["state"], "ready");
    assert_eq!(json["sources"][0]["inspectedEntries"], 1);
    assert_eq!(json["project"]["inspectedEntries"], 1);
    assert_eq!(
        serde_json::to_value(source(
            HarnessName::LoopEngineering,
            Vendor::Codex,
            Scope::Global
        ))
        .unwrap()["harness"],
        "loop-engineering"
    );
    assert_eq!(
        serde_json::to_value(DocumentOwner::Skill {
            source: source(HarnessName::WorkflowHarness, Vendor::Claude, Scope::Project)
        })
        .unwrap()["kind"],
        "skill"
    );
}

#[test]
fn returns_explicit_empty_and_read_errors() {
    let project = fixture("errors");
    let home = fixture("errors-home");
    write(
        &project,
        ".claude/skills/workflow-harness/SKILL.md",
        "skill",
    );
    let ListResult::Ready { sources, .. } = list(&project, &home, HarnessName::WorkflowHarness)
    else {
        panic!("ready")
    };
    assert!(matches!(sources[0], SourceListing::Empty { .. }));
    assert!(matches!(
        read(&project, &home, owner(&project), "memory/missing.md"),
        ReadResult::Error {
            error: ReadErrorOrRequest::Read(ReadError::NotFound)
        }
    ));
    let wrong = fixture("wrong");
    assert!(matches!(
        read(&project, &home, owner(&wrong), "lessons-seed"),
        ReadResult::Error {
            error: ReadErrorOrRequest::Request(RequestError::InvalidRequest)
        }
    ));
}
