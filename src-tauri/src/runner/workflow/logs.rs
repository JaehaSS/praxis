//! Immutable bounded log blobs. The SQLite receipts authorize access to each digest.
use super::config::hash;
use anyhow::{ensure, Result};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

fn nonce() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("OS randomness unavailable");
    hash(&bytes)
}

pub const MAX_LOG_BYTES: usize = 10 * 1024 * 1024;

pub fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "log is not a regular file"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(file.metadata()?.is_file(), "log is not a regular file");
    let mut bytes = Vec::new();
    file.take(MAX_LOG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_LOG_BYTES, "log exceeds 10 MiB");
    Ok(bytes)
}

pub fn publish(workspace: &Path, bytes: &[u8]) -> Result<String> {
    ensure!(bytes.len() <= MAX_LOG_BYTES, "log exceeds 10 MiB");
    let digest = hash(bytes);
    let root = workspace.join("logs");
    fs::create_dir_all(&root)?;
    let final_path = root.join(&digest);
    if final_path.exists() {
        ensure!(
            read(workspace, &digest)? == bytes,
            "existing log was altered"
        );
        return Ok(digest);
    }
    let temp = root.join(format!(".{}", nonce()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    let result = fs::hard_link(&temp, &final_path);
    fs::remove_file(&temp)?;
    if let Err(error) = result {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    File::open(&root)?.sync_all()?;
    ensure!(
        read(workspace, &digest)? == bytes,
        "published log digest mismatch"
    );
    Ok(digest)
}

pub fn read(workspace: &Path, digest: &str) -> Result<Vec<u8>> {
    ensure!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid log digest"
    );
    let bytes = read_bounded(&workspace.join("logs").join(digest))?;
    ensure!(hash(&bytes) == digest, "stored log digest mismatch");
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn logs_are_bounded_deduplicated_and_rehashed() {
        let root = std::env::temp_dir().join(format!("praxis-log-test-{}", nonce()));
        let bytes = vec![b'x'; 2 * 1024 * 1024];
        let digest = publish(&root, &bytes).unwrap();
        assert_eq!(publish(&root, &bytes).unwrap(), digest);
        assert_eq!(read(&root, &digest).unwrap(), bytes);
        assert!(read(&root, "../../secrets").is_err());
        assert!(publish(&root, &vec![0; MAX_LOG_BYTES + 1]).is_err());
        fs::write(root.join("logs").join(&digest), b"tampered").unwrap();
        assert!(read(&root, &digest).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
