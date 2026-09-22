use std::io::Read;
use std::path::{Path, PathBuf};

use crate::knowledge::graph::Document;

use super::config::WikiSpaceEntry;
use super::{MAX_BODY_BYTES, SOURCE_ID};

pub struct ScanResult {
    pub documents: Vec<Document>,
    pub warnings: Vec<String>,
    pub complete: bool,
}

pub fn scan(space: &WikiSpaceEntry, root: &Path) -> ScanResult {
    let mut result = ScanResult {
        documents: Vec::new(),
        warnings: Vec::new(),
        complete: true,
    };
    walk(space, root, root, &mut result);
    result
        .documents
        .sort_by(|a, b| a.external_id.cmp(&b.external_id));
    result
}

pub fn external_id(space_id: &str, relative_path: &str) -> String {
    format!("{space_id}/{relative_path}")
}

pub fn relative_path(space_id: &str, external_id: &str) -> anyhow::Result<String> {
    external_id
        .strip_prefix(&format!("{space_id}/"))
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("indexed document does not belong to its Wiki space"))
}

pub fn excluded(relative: &str, patterns: &[String]) -> bool {
    relative
        .split('/')
        .any(|part| matches!(part, ".git" | ".obsidian" | ".trash"))
        || crate::knowledge::source::obsidian::is_excluded(relative, patterns)
}

pub fn read(root: &Path, relative: &str, patterns: &[String]) -> anyhow::Result<(PathBuf, String)> {
    if excluded(relative, patterns) {
        anyhow::bail!("indexed document is excluded from Wiki")
    }
    let absolute_relative = root
        .strip_prefix(Path::new("/"))?
        .join(relative)
        .to_string_lossy()
        .into_owned();
    let file = crate::evidence::scoped_file::open(Path::new("/"), &absolute_relative)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        anyhow::bail!("Wiki document is not a readable regular file")
    }
    if metadata.len() > MAX_BODY_BYTES {
        anyhow::bail!("Wiki document exceeds the 2 MiB read limit")
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize + 1);
    file.take(MAX_BODY_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BODY_BYTES {
        anyhow::bail!("Wiki document exceeds the 2 MiB read limit")
    }
    Ok((root.join(relative), String::from_utf8(bytes)?))
}

fn walk(space: &WikiSpaceEntry, root: &Path, directory: &Path, result: &mut ScanResult) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            result.complete = false;
            result
                .warnings
                .push(format!("cannot scan {}: {error}", directory.display()));
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            result.complete = false;
            result
                .warnings
                .push("cannot inspect a Wiki directory entry".into());
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            result.complete = false;
            result
                .warnings
                .push(format!("cannot inspect {}", entry.path().display()));
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let Some(relative) = to_relative(root, &path) else {
            result.complete = false;
            result
                .warnings
                .push(format!("outside Wiki root: {}", path.display()));
            continue;
        };
        if excluded(&relative, &space.exclude) {
            continue;
        }
        if kind.is_dir() {
            walk(space, root, &path, result);
        } else if kind.is_file() && markdown(&path) {
            match read(root, &relative, &space.exclude) {
                Ok((_, body)) => result
                    .documents
                    .push(document(space, &relative, body, &path)),
                Err(error) => {
                    result.complete = false;
                    result
                        .warnings
                        .push(format!("cannot read {}: {error}", path.display()));
                }
            }
        }
    }
}

fn document(space: &WikiSpaceEntry, relative: &str, body: String, path: &Path) -> Document {
    let updated_at = std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let title = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| relative.to_owned());
    Document {
        source: SOURCE_ID.into(),
        external_id: external_id(&space.id, relative),
        kind: "document".into(),
        title,
        url: Some(format!("file://{}", path.display())),
        body,
        updated_at,
        embed: !crate::knowledge::source::obsidian::is_excluded(relative, &space.embed_exclude),
    }
}

fn to_relative(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root).ok().map(|relative| {
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    })
}

fn markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}
