use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::RootIdentity;

pub fn verified_root(path: &Path) -> anyhow::Result<RootIdentity> {
    super::require_supported()?;
    let canonical = path.canonicalize()?;
    let metadata = std::fs::metadata(&canonical)?;
    if !metadata.is_dir() {
        anyhow::bail!("vault root must be a directory")
    }
    Ok(RootIdentity {
        canonical_root: canonical.to_string_lossy().into_owned(),
        device: metadata.dev() as i64,
        inode: metadata.ino() as i64,
    })
}
