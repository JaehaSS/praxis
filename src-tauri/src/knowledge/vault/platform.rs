use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod unsupported;

#[derive(Debug, Clone)]
pub struct RootIdentity {
    pub canonical_root: String,
    pub device: i64,
    pub inode: i64,
}

#[cfg(target_os = "macos")]
pub fn verified_root(path: &Path) -> anyhow::Result<RootIdentity> {
    macos::verified_root(path)
}

#[cfg(not(target_os = "macos"))]
pub fn verified_root(path: &Path) -> anyhow::Result<RootIdentity> {
    unsupported::verified_root(path)
}

pub fn require_supported() -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    return Ok(());
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("unsupported_platform")
}
