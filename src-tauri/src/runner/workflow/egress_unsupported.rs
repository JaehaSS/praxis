//! Non-Unix desktops can compile the API surface but cannot create workflow egress.
use anyhow::{bail, Result};
use std::path::PathBuf;

pub struct EgressPolicy;
impl EgressPolicy {
    pub fn new(_: impl IntoIterator<Item = String>, _: usize) -> Result<Self> {
        bail!("capability_unavailable: workflow egress requires a Linux Runner")
    }
}
pub struct EgressProxy;
impl EgressProxy {
    pub async fn start(_: PathBuf, _: EgressPolicy) -> Result<Self> {
        bail!("capability_unavailable: workflow egress requires a Linux Runner")
    }
    pub fn successful_connects(&self) -> u64 {
        0
    }
    pub async fn shutdown(self) -> Result<()> {
        Ok(())
    }
}
