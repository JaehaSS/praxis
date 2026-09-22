use std::path::Path;

use super::RootIdentity;

pub fn verified_root(_path: &Path) -> anyhow::Result<RootIdentity> {
    anyhow::bail!("unsupported_platform")
}
