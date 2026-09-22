use super::*;
use crate::evidence::scoped_file;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_DOCUMENT_BYTES: u64 = 128 * 1024;
const GENERATED_MARKER: &str = "<!-- 생성물: npm run docs:project — 손으로 고치지 마시오 -->";
const GENERATED_TITLE: &str = "# Praxis — 교훈 (전 기간)";

pub fn read(
    project_root: &Path,
    home: &Path,
    owner: DocumentOwner,
    document_key: &str,
) -> ReadResult {
    #[cfg(windows)]
    return ReadResult::Unsupported {
        reason: UnsupportedReason::SecureReadUnavailable,
    };

    #[cfg(not(windows))]
    match resolve(project_root, home, &owner, document_key) {
        Err(error) => ReadResult::Error { error },
        Ok((root, relative)) => match read_relative(&root, &relative, || {}) {
            Err(error) => ReadResult::Error {
                error: ReadErrorOrRequest::Read(error),
            },
            Ok((text, content_hash)) => {
                let (generated, source_resolution) = generated(&root, document_key, &text);
                ReadResult::Ready {
                    document: ExperienceDocument {
                        owner,
                        document_key: document_key.to_string(),
                        path: root.join(relative).to_string_lossy().to_string(),
                        content_hash,
                        observed_at: chrono::Utc::now().to_rfc3339(),
                        generated,
                        source_resolution,
                        text,
                    },
                }
            }
        },
    }
}

fn resolve(
    project_root: &Path,
    home: &Path,
    owner: &DocumentOwner,
    key: &str,
) -> Result<(PathBuf, String), ReadErrorOrRequest> {
    match owner {
        DocumentOwner::Skill { source } if key == "skill-lessons" => {
            let root = if source.scope == Scope::Project {
                project_root
            } else {
                home
            };
            let skill = super::discovery::skill_relative(source);
            let relative = format!("{}/LESSONS_LEARNED.md", skill.trim_end_matches("/SKILL.md"));
            Ok((root.to_path_buf(), relative))
        }
        DocumentOwner::Project { project }
            if project.host == Host::Local
                && project.project_key == project_root.to_string_lossy() =>
        {
            project_relative(key)
                .map(|relative| (project_root.to_path_buf(), relative))
                .ok_or(ReadErrorOrRequest::Request(RequestError::InvalidRequest))
        }
        _ => Err(ReadErrorOrRequest::Request(RequestError::InvalidRequest)),
    }
}

fn project_relative(key: &str) -> Option<String> {
    if key == "project-lessons" {
        return Some("LESSONS_LEARNED.md".to_string());
    }
    if key == "lessons-seed" {
        return Some("docs/lessons-seed.md".to_string());
    }
    let (directory, basename) = key.split_once('/')?;
    if !matches!(directory, "memory" | "archive") || !valid_basename(basename) {
        return None;
    }
    Some(format!("docs/{directory}/{basename}"))
}

fn valid_basename(name: &str) -> bool {
    !name.is_empty()
        && name.ends_with(".md")
        && name != ".md"
        && !name.starts_with('.')
        && !name.contains(['/', '\\', '\0'])
        && name != "."
        && name != ".."
}

pub(super) fn read_relative(
    root: &Path,
    relative: &str,
    after_read: impl FnOnce(),
) -> Result<(String, String), ReadError> {
    let mut file = scoped_file::open(root, relative).map_err(super::discovery::classify_io)?;
    let before = file.metadata().map_err(super::discovery::classify_io)?;
    if !before.is_file() {
        return Err(ReadError::NotRegular);
    }
    if before.len() > MAX_DOCUMENT_BYTES {
        return Err(ReadError::Oversized);
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.by_ref()
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(super::discovery::classify_io)?;
    after_read();
    let after = file.metadata().map_err(super::discovery::classify_io)?;
    if !same_file(&before, &after) {
        return Err(ReadError::Changed);
    }
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(ReadError::Oversized);
    }
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let text = String::from_utf8(bytes).map_err(|_| ReadError::InvalidUtf8)?;
    Ok((text, hash))
}

#[cfg(unix)]
fn same_file(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

#[cfg(not(unix))]
fn same_file(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}

fn generated(root: &Path, key: &str, text: &str) -> (bool, SourceResolution) {
    let mut lines = text.lines();
    if key != "project-lessons"
        || lines.next() != Some(GENERATED_MARKER)
        || lines.find(|line| !line.is_empty()) != Some(GENERATED_TITLE)
    {
        return (false, SourceResolution::NotApplicable);
    }
    let sources_available = scoped_file::list(root, "docs/memory", 0).is_ok()
        && scoped_file::list(root, "docs/archive", 0).is_ok()
        && super::discovery::regular(root, "docs/lessons-seed.md").unwrap_or(false);
    if sources_available {
        (true, SourceResolution::KnownSet)
    } else {
        (false, SourceResolution::Unconfirmed)
    }
}
