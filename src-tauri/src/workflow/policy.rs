//! Syntax policies shared by workflow import, resource reservation, and artifact collection.

use std::path::{Component, Path};

pub const MAX_PATH_BYTES: usize = 512;

/// A registered, canonical repository key, not a filesystem path or Git URL.
/// The future runtime admission layer must resolve aliases to this key before
/// authorizing a plan; the domain library cannot infer repository ownership.
pub fn validate_project_ref(value: &str) -> Result<(), String> {
    validate_safe_path(value)?;
    if value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
    {
        return Err("project_ref must be a bounded canonical repository key".into());
    }
    Ok(())
}

/// Validates the v1 repository-relative file or directory-prefix syntax.
///
/// Paths are deliberately not normalized: accepting an alternate spelling would make a
/// reservation for `a/../b` differ from the actual file that a worker changes.
pub fn validate_safe_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.len() > MAX_PATH_BYTES {
        return Err("path must be a non-empty, trimmed repository-relative value".into());
    }
    if value.starts_with(['/', '\\'])
        || value.contains('\\')
        || value.as_bytes().get(1) == Some(&b':')
        || value.contains('\0')
        || value.contains(['*', '?', '[', ']', '{', '}', '!'])
        || value.ends_with('/')
    {
        return Err("path must not be absolute, use a Windows prefix, or contain globs".into());
    }
    if value
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(
            "path must not contain empty, current-directory, or parent-directory segments".into(),
        );
    }
    if !Path::new(value)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(
            "path must not contain empty, current-directory, or parent-directory segments".into(),
        );
    }
    Ok(())
}

/// Returns true when two valid v1 exact-file/directory-prefix paths overlap.
pub fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(crate) fn validate_resource_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > 256
        || value.contains('\0')
        || value.contains(['\n', '\r', '\\', '*', '?', '[', ']', '{', '}'])
    {
        return Err("resource_id must be a trimmed, bounded registered resource identifier".into());
    }
    Ok(())
}
