use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const MAX_EVIDENCE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum ObserveError {
    Missing,
    Invalid(String),
    Unknown(String),
}

impl std::fmt::Display for ObserveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(formatter, "evidence source is missing"),
            Self::Invalid(message) | Self::Unknown(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ObserveError {}

pub(crate) struct CodeSnapshot {
    pub hash: String,
    pub commit_oid: String,
    pub canonical_root: PathBuf,
}

pub(crate) fn code(
    root: &Path,
    relative_path: &str,
    line_start: u32,
    line_end: u32,
) -> Result<CodeSnapshot, ObserveError> {
    if line_start == 0 || line_end < line_start {
        return Err(ObserveError::Invalid("invalid inclusive line range".into()));
    }
    let canonical_root = canonical_root(root)?;
    let bytes = read_scoped(&canonical_root, relative_path)?;
    let selected = select_lines(&bytes, line_start, line_end)?;
    let commit_oid = commit_oid(&canonical_root)?;
    Ok(CodeSnapshot {
        hash: sha256(selected),
        commit_oid,
        canonical_root,
    })
}

pub(crate) fn document(root: &Path, relative_path: &str) -> Result<String, ObserveError> {
    let canonical_root = canonical_root(root)?;
    read_scoped(&canonical_root, relative_path).map(sha256)
}

pub(crate) fn external_url(raw: &str) -> Result<String, ObserveError> {
    let Some(authority) = raw.strip_prefix("https://") else {
        return Err(ObserveError::Invalid(
            "external document requires credential-free HTTPS".into(),
        ));
    };
    if authority.is_empty() || authority.starts_with('/') {
        return Err(ObserveError::Invalid(
            "external document URL requires an explicit host".into(),
        ));
    }
    let url = reqwest::Url::parse(raw)
        .map_err(|_| ObserveError::Invalid("external document URL is invalid".into()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ObserveError::Invalid(
            "external document requires credential-free HTTPS".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ObserveError::Invalid(
            "external document URL cannot contain query or fragment".into(),
        ));
    }
    Ok(url.to_string())
}

fn canonical_root(root: &Path) -> Result<PathBuf, ObserveError> {
    root.canonicalize().map_err(classify_io)
}

fn read_scoped(root: &Path, relative_path: &str) -> Result<Vec<u8>, ObserveError> {
    let mut file = super::scoped_file::open(root, relative_path).map_err(classify_io)?;
    let metadata = file.metadata().map_err(classify_io)?;
    if !metadata.is_file() {
        return Err(ObserveError::Invalid(
            "evidence source is not a regular file".into(),
        ));
    }
    if metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(ObserveError::Invalid("evidence source is too large".into()));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_EVIDENCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(classify_io)?;
    if bytes.len() as u64 > MAX_EVIDENCE_BYTES {
        return Err(ObserveError::Invalid("evidence source is too large".into()));
    }
    Ok(bytes)
}

fn select_lines(bytes: &[u8], start: u32, end: u32) -> Result<&[u8], ObserveError> {
    let mut line = 1_u32;
    let mut begin = None;
    for (index, byte) in bytes.iter().enumerate() {
        if line == start && begin.is_none() {
            begin = Some(index);
        }
        if *byte == b'\n' {
            if line == end {
                return Ok(&bytes[begin.unwrap_or(index)..=index]);
            }
            line += 1;
        }
    }
    if line == start && begin.is_none() {
        begin = Some(bytes.len());
    }
    if line >= end {
        return Ok(&bytes[begin.unwrap_or(bytes.len())..]);
    }
    Err(ObserveError::Invalid(
        "line range exceeds evidence source".into(),
    ))
}

fn commit_oid(root: &Path) -> Result<String, ObserveError> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .map_err(classify_io)?;
    if !output.status.success() {
        return Err(ObserveError::Invalid(
            "repository has no commit identity".into(),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| ObserveError::Unknown("git returned non-UTF-8 commit identity".into()))
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(bytes.as_ref());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn classify_io(error: std::io::Error) -> ObserveError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return ObserveError::Missing;
    }
    ObserveError::Unknown(error.to_string())
}
