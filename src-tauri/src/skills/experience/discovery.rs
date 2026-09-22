use super::*;
use crate::evidence::scoped_file;
use std::path::Path;

const DIRECTORY_LIMIT: usize = 200;

pub fn list(project_root: &Path, home: &Path, harness: HarnessName) -> ListResult {
    #[cfg(windows)]
    return ListResult::Unsupported {
        reason: UnsupportedReason::SecureReadUnavailable,
    };

    #[cfg(not(windows))]
    {
        let project = ProjectRef {
            host: Host::Local,
            project_key: project_root.to_string_lossy().to_string(),
        };
        let sources = [Vendor::Claude, Vendor::Codex]
            .into_iter()
            .flat_map(|vendor| {
                let harness = harness.clone();
                [Scope::Project, Scope::Global]
                    .into_iter()
                    .map(move |scope| {
                        source_listing(project_root, home, harness.clone(), vendor.clone(), scope)
                    })
            })
            .collect();
        ListResult::Ready {
            observed_at: chrono::Utc::now().to_rfc3339(),
            sources,
            project: project_listing(project_root, project),
        }
    }
}

fn source_listing(
    project_root: &Path,
    home: &Path,
    harness: HarnessName,
    vendor: Vendor,
    scope: Scope,
) -> SourceListing {
    let source = SourceRef {
        host: Host::Local,
        harness,
        vendor,
        scope: scope.clone(),
    };
    let root = if scope == Scope::Project {
        project_root
    } else {
        home
    };
    let skill = skill_relative(&source);
    match regular(root, &skill) {
        Ok(false) => SourceListing::NotInstalled { source },
        Err(error) => SourceListing::Error { source, error },
        Ok(true) => {
            let lessons = format!("{}/LESSONS_LEARNED.md", skill.trim_end_matches("/SKILL.md"));
            match regular(root, &lessons) {
                Ok(false) => SourceListing::Empty { source },
                Err(error) => SourceListing::Error { source, error },
                Ok(true) => SourceListing::Ready {
                    source,
                    documents: vec![descriptor(
                        root,
                        "skill-lessons",
                        &lessons,
                        Relation::HarnessOwned,
                    )],
                    limited: false,
                    inspected_entries: 1,
                },
            }
        }
    }
}

fn project_listing(root: &Path, project: ProjectRef) -> ProjectListing {
    match collect_project_documents(root) {
        Ok((documents, _, _)) if documents.is_empty() => ProjectListing::Empty { project },
        Ok((documents, limited, inspected_entries)) => ProjectListing::Ready {
            project,
            documents,
            limited,
            inspected_entries,
        },
        Err(error) => ProjectListing::Error { project, error },
    }
}

fn collect_project_documents(
    root: &Path,
) -> Result<(Vec<DocumentDescriptor>, bool, usize), ReadError> {
    let mut documents = Vec::new();
    if regular(root, "LESSONS_LEARNED.md")? {
        documents.push(descriptor(
            root,
            "project-lessons",
            "LESSONS_LEARNED.md",
            Relation::ProjectReference,
        ));
    }
    if regular(root, "docs/lessons-seed.md")? {
        documents.push(descriptor(
            root,
            "lessons-seed",
            "docs/lessons-seed.md",
            Relation::ProjectReference,
        ));
    }
    let mut limited = false;
    let mut inspected_entries = 0;
    for (directory, prefix) in [("docs/memory", "memory"), ("docs/archive", "archive")] {
        match markdown_children(root, directory, prefix) {
            Ok((mut listed, was_limited, inspected)) => {
                documents.append(&mut listed);
                limited |= was_limited;
                inspected_entries += inspected;
            }
            Err(ReadError::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    documents.sort_by(|left, right| left.key.cmp(&right.key));
    Ok((documents, limited, inspected_entries))
}

fn markdown_children(
    root: &Path,
    directory: &str,
    prefix: &str,
) -> Result<(Vec<DocumentDescriptor>, bool, usize), ReadError> {
    let entries = scoped_file::list(root, directory, DIRECTORY_LIMIT).map_err(classify_io)?;
    let mut documents = Vec::new();
    for name in entries.names {
        let Some(name) = name.to_str() else { continue };
        if name.starts_with('.') || !name.ends_with(".md") || name.len() == 3 {
            continue;
        }
        let relative = format!("{directory}/{name}");
        if regular(root, &relative)? {
            documents.push(descriptor(
                root,
                &format!("{prefix}/{name}"),
                &relative,
                Relation::ProjectReference,
            ));
        }
    }
    Ok((documents, entries.limited, entries.inspected_entries))
}

pub(super) fn skill_relative(source: &SourceRef) -> String {
    let vendor = match &source.vendor {
        Vendor::Claude => ".claude",
        Vendor::Codex => ".codex",
    };
    let harness = match &source.harness {
        HarnessName::WorkflowHarness => "workflow-harness",
        HarnessName::LoopEngineering => "loop-engineering",
    };
    format!("{vendor}/skills/{harness}/SKILL.md")
}

pub(super) fn descriptor(
    root: &Path,
    key: &str,
    path: &str,
    relation: Relation,
) -> DocumentDescriptor {
    DocumentDescriptor {
        key: key.to_string(),
        display_path: root.join(path).to_string_lossy().to_string(),
        relation,
    }
}

pub(super) fn regular(root: &Path, relative: &str) -> Result<bool, ReadError> {
    match scoped_file::open(root, relative) {
        Ok(file) => file
            .metadata()
            .map(|metadata| metadata.is_file())
            .map_err(classify_io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(classify_io(error)),
    }
}

pub(super) fn classify_io(error: std::io::Error) -> ReadError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ReadError::NotFound,
        std::io::ErrorKind::PermissionDenied => ReadError::PermissionDenied,
        std::io::ErrorKind::InvalidInput => ReadError::UnsafePath,
        #[cfg(unix)]
        _ if error.raw_os_error() == Some(nix::libc::ELOOP) => ReadError::UnsafePath,
        _ => ReadError::IoError,
    }
}
