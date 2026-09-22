mod config;
mod embedding;
mod legacy;
mod query;
mod sync;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

pub(super) fn directory(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path =
        crate::testtmp::dir().join(format!("praxis-wiki-{name}-{}-{count}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}
