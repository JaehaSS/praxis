//! Confinement, optimistic concurrency, and rollback for context-file projection.

use std::io::{Error, ErrorKind};
use std::path::Path;

use serde::{Deserialize, Serialize};

mod io;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetPreimage {
    pub relative_path: String,
    pub content: Option<String>,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub unix_mode: Option<u32>,
}

pub fn capture_targets(worktree: &Path, targets: &[&str]) -> std::io::Result<Vec<TargetPreimage>> {
    targets
        .iter()
        .map(|relative| io::read_preimage(worktree, relative))
        .collect()
}

fn expected_mode(preimage: &TargetPreimage, update: &Option<String>) -> Option<u32> {
    if update.is_none() {
        return None;
    }
    preimage.unix_mode.or(io::new_file_mode())
}

fn permission_bits(mode: Option<u32>) -> Option<u32> {
    mode.map(|value| value & 0o7777)
}

fn matches_postimage(
    current: &TargetPreimage,
    preimage: &TargetPreimage,
    update: &Option<String>,
) -> bool {
    current.content == *update
        && current.readonly == preimage.readonly
        && permission_bits(current.unix_mode) == permission_bits(expected_mode(preimage, update))
}

pub fn restore_matching(
    worktree: &Path,
    preimages: &[TargetPreimage],
    postimages: &[Option<String>],
) -> std::io::Result<()> {
    if preimages.len() != postimages.len() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "rollback plan length mismatch",
        ));
    }
    let targets = preimages
        .iter()
        .map(|preimage| preimage.relative_path.as_str())
        .collect::<Vec<_>>();
    let current = capture_targets(worktree, &targets)?;
    for ((now, before), after) in current.iter().zip(preimages).zip(postimages) {
        if now != before && !matches_postimage(now, before, after) {
            return Err(Error::new(
                ErrorKind::WouldBlock,
                format!(
                    "projection target changed independently: {}",
                    before.relative_path
                ),
            ));
        }
    }
    for ((now, before), after) in current.iter().zip(preimages).zip(postimages).rev() {
        if now != before && matches_postimage(now, before, after) {
            let path = io::target_path(worktree, &before.relative_path)?;
            io::write_update(&path, before, &before.content)?;
        }
    }
    Ok(())
}

fn validate_plan(
    worktree: &Path,
    preimages: &[TargetPreimage],
    updates: &[Option<String>],
) -> std::io::Result<()> {
    if preimages.len() != updates.len() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "projection plan length mismatch",
        ));
    }
    let targets = preimages
        .iter()
        .map(|preimage| preimage.relative_path.as_str())
        .collect::<Vec<_>>();
    if capture_targets(worktree, &targets)? != preimages {
        return Err(Error::new(
            ErrorKind::WouldBlock,
            "projection target changed",
        ));
    }
    for update in updates.iter().flatten() {
        if update.len() as u64 > io::MAX_TARGET_BYTES {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "projected target is too large",
            ));
        }
    }
    Ok(())
}

fn apply_updates_with<F>(
    worktree: &Path,
    preimages: &[TargetPreimage],
    updates: &[Option<String>],
    mut writer: F,
) -> std::io::Result<()>
where
    F: FnMut(&Path, &TargetPreimage, &Option<String>) -> std::io::Result<()>,
{
    validate_plan(worktree, preimages, updates)?;
    for (preimage, update) in preimages.iter().zip(updates) {
        let path = io::target_path(worktree, &preimage.relative_path)?;
        if let Err(error) = writer(&path, preimage, update) {
            if let Err(rollback) = restore_matching(worktree, preimages, updates) {
                return Err(Error::other(format!(
                    "projection failed: {error}; rollback failed: {rollback}"
                )));
            }
            return Err(error);
        }
    }
    Ok(())
}

pub fn apply_updates(
    worktree: &Path,
    preimages: &[TargetPreimage],
    updates: &[Option<String>],
) -> std::io::Result<()> {
    apply_updates_with(worktree, preimages, updates, io::write_update)
}

#[cfg(test)]
mod tests;
