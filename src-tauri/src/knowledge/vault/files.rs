use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

use super::catalog::{create_document, DocumentDraft, VaultDocument};
use super::operations::{
    commit_operation, mark_files_ready, prepare_operation, prepare_operation_with_metadata,
    write_operation_file, write_operation_reader, OperationPlan,
};
use super::platform;
use super::scope::ScopeRequest;

pub const MAX_IMPORT_BYTES: u64 = 100 * 1024 * 1024;
const MAX_BATCH_FILES: usize = 20;
const MAX_BATCH_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub vault_id: String,
    pub source: PathBuf,
    pub title: String,
    pub scope: ScopeRequest,
}

#[derive(Debug, Clone)]
pub struct ImportedFile {
    pub document: VaultDocument,
    pub revision_id: String,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct TextSourceDraft {
    pub vault_id: String,
    pub title: String,
    pub body: String,
    pub scope: ScopeRequest,
}

#[derive(Debug, Clone)]
pub struct UrlSourceDraft {
    pub vault_id: String,
    pub title: String,
    pub url: String,
    pub memo: String,
    pub scope: ScopeRequest,
}

pub async fn import_file(
    pool: &SqlitePool,
    request: &ImportRequest,
    now: i64,
) -> anyhow::Result<ImportedFile> {
    let _admission = super::shared_admission(pool).await?;
    let root = vault_root(pool, &request.vault_id).await?;
    if fs::symlink_metadata(&request.source)?
        .file_type()
        .is_symlink()
    {
        anyhow::bail!("vault imports require a regular non-symlink file")
    }
    let source = request.source.canonicalize()?;
    if super::scan::excluded(&source) {
        anyhow::bail!("vault import path is excluded")
    }
    let relative = source.strip_prefix(&root.path).ok().and_then(relative_path);
    let mut file = match &relative {
        Some(relative) => open_scoped_verified(&root.path, root.device, root.inode, relative)?,
        None => open_import(&source)?,
    };
    ensure_regular(&file)?;
    let (digest, size) = hash_import(&mut file)?;
    if let Some(row) = sqlx::query("SELECT d.id, r.id AS revision_id FROM vault_documents d JOIN vault_revisions r ON r.id = d.current_revision WHERE d.vault_id = ? AND d.kind = 'source' AND d.state = 'active' AND d.title = ? AND r.sha256 = ?")
        .bind(&request.vault_id).bind(&request.title).bind(&digest).fetch_optional(pool).await? {
        let revision_id: String = row.try_get("revision_id")?;
        if verify_revision(pool, &revision_id).await.is_ok() {
            let document = super::catalog::get_document(pool, &row.try_get::<String, _>("id")?).await?.ok_or_else(|| anyhow::anyhow!("vault duplicate document is missing"))?;
            return Ok(ImportedFile { document, revision_id, sha256: digest })
        }
    }
    let document = create_document(
        pool,
        &DocumentDraft {
            vault_id: request.vault_id.clone(),
            kind: "source".into(),
            title: request.title.clone(),
        },
        now,
    )
    .await?;
    let revision_id = super::catalog::identifier("revision")?;
    let target = relative.unwrap_or_else(|| {
        format!(
            "sources/{}/{revision_id}/{}",
            document.id,
            safe_name(&source).unwrap_or_else(|_| "source".into())
        )
    });
    let plan = OperationPlan::new(
        request.vault_id.clone(),
        document.id.clone(),
        None,
        revision_id.clone(),
        target,
        Vec::new(),
        request.scope.clone(),
    );
    let operation = prepare_operation_with_metadata(pool, &plan, &digest, size, now).await?;
    if source.starts_with(&root.path) {
        mark_files_ready(pool, &operation.id).await?;
    } else {
        file.seek(SeekFrom::Start(0))?;
        write_operation_reader(pool, &operation, &mut file, size).await?;
    }
    commit_operation(pool, &operation.id, now).await?;
    let _ = super::index::index_revision(pool, &revision_id).await;
    Ok(ImportedFile {
        document,
        revision_id,
        sha256: operation.sha256,
    })
}

pub async fn import_batch(
    pool: &SqlitePool,
    requests: &[ImportRequest],
    now: i64,
) -> anyhow::Result<Vec<Result<ImportedFile, String>>> {
    if requests.len() > MAX_BATCH_FILES {
        anyhow::bail!("vault import exceeds 20 files")
    }
    let total = requests
        .iter()
        .filter_map(|request| {
            fs::metadata(&request.source)
                .ok()
                .map(|metadata| metadata.len())
        })
        .sum::<u64>();
    if total > MAX_BATCH_BYTES {
        anyhow::bail!("vault import exceeds 200 MiB")
    }
    let mut results = Vec::with_capacity(requests.len());
    for request in requests {
        results.push(
            import_file(pool, request, now)
                .await
                .map_err(|error| error.to_string()),
        );
    }
    Ok(results)
}

pub async fn create_text_source(
    pool: &SqlitePool,
    draft: &TextSourceDraft,
    now: i64,
) -> anyhow::Result<ImportedFile> {
    save_manual_source(
        pool,
        &draft.vault_id,
        "source",
        &draft.title,
        "text.md",
        draft.body.as_bytes(),
        &draft.scope,
        now,
    )
    .await
}

pub async fn create_url_source(
    pool: &SqlitePool,
    draft: &UrlSourceDraft,
    now: i64,
) -> anyhow::Result<ImportedFile> {
    if draft.url.trim().is_empty() {
        anyhow::bail!("vault URL is required")
    }
    let body = format!("URL: {}\n\n{}", draft.url, draft.memo);
    save_manual_source(
        pool,
        &draft.vault_id,
        "url",
        &draft.title,
        "url.md",
        body.as_bytes(),
        &draft.scope,
        now,
    )
    .await
}

async fn save_manual_source(
    pool: &SqlitePool,
    vault_id: &str,
    kind: &str,
    title: &str,
    name: &str,
    content: &[u8],
    scope: &ScopeRequest,
    now: i64,
) -> anyhow::Result<ImportedFile> {
    let _admission = super::shared_admission(pool).await?;
    if content.len() as u64 > MAX_IMPORT_BYTES {
        anyhow::bail!("vault import exceeds 100 MiB")
    }
    vault_root(pool, vault_id).await?;
    let document = create_document(
        pool,
        &DocumentDraft {
            vault_id: vault_id.into(),
            kind: kind.into(),
            title: title.into(),
        },
        now,
    )
    .await?;
    let revision_id = super::catalog::identifier("revision")?;
    let target = format!("sources/{}/{revision_id}/{name}", document.id);
    let plan = OperationPlan::new(
        vault_id.into(),
        document.id.clone(),
        None,
        revision_id.clone(),
        target,
        content.to_vec(),
        scope.clone(),
    );
    let operation = prepare_operation(pool, &plan, now).await?;
    write_operation_file(pool, &operation, &plan.content).await?;
    commit_operation(pool, &operation.id, now).await?;
    let _ = super::index::index_revision(pool, &revision_id).await;
    Ok(ImportedFile {
        document,
        revision_id,
        sha256: operation.sha256,
    })
}

pub async fn read_revision(pool: &SqlitePool, revision_id: &str) -> anyhow::Result<Vec<u8>> {
    let row = sqlx::query("SELECT v.canonical_root, v.root_device, v.root_inode, r.relative_path, r.sha256 FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id JOIN vaults v ON v.id = d.vault_id WHERE r.id = ?")
        .bind(revision_id).fetch_optional(pool).await?.ok_or_else(|| anyhow::anyhow!("vault revision is missing"))?;
    let root: String = row.try_get("canonical_root")?;
    let device: i64 = row.try_get("root_device")?;
    let inode: i64 = row.try_get("root_inode")?;
    let identity = platform::verified_root(Path::new(&root))?;
    if identity.canonical_root != root || identity.device != device || identity.inode != inode {
        anyhow::bail!("vault root identity changed")
    }
    let bytes = read_scoped_verified(
        Path::new(&root),
        device,
        inode,
        &row.try_get::<String, _>("relative_path")?,
    )?;
    if hash(&bytes) != row.try_get::<String, _>("sha256")? {
        anyhow::bail!("vault revision drifted")
    }
    Ok(bytes)
}

pub(crate) async fn verify_revision(pool: &SqlitePool, revision_id: &str) -> anyhow::Result<()> {
    let row = sqlx::query("SELECT v.canonical_root, v.root_device, v.root_inode, r.relative_path, r.sha256, r.size FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id JOIN vaults v ON v.id = d.vault_id WHERE r.id = ?")
        .bind(revision_id).fetch_optional(pool).await?.ok_or_else(|| anyhow::anyhow!("vault revision is missing"))?;
    let root: String = row.try_get("canonical_root")?;
    let identity = platform::verified_root(Path::new(&root))?;
    if identity.canonical_root != root
        || identity.device != row.try_get::<i64, _>("root_device")?
        || identity.inode != row.try_get::<i64, _>("root_inode")?
    {
        anyhow::bail!("vault root identity changed")
    }
    let file = open_scoped_verified(
        Path::new(&root),
        row.try_get("root_device")?,
        row.try_get("root_inode")?,
        &row.try_get::<String, _>("relative_path")?,
    )?;
    ensure_regular(&file)?;
    if file.metadata()?.len() != row.try_get::<i64, _>("size")? as u64 {
        anyhow::bail!("vault revision drifted")
    }
    if hash_reader(file)? != row.try_get::<String, _>("sha256")? {
        anyhow::bail!("vault revision drifted")
    }
    Ok(())
}

pub async fn verified_original_path(
    pool: &SqlitePool,
    revision_id: &str,
) -> anyhow::Result<PathBuf> {
    let row = sqlx::query("SELECT v.canonical_root, v.root_device, v.root_inode, r.relative_path, r.sha256 FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id JOIN vaults v ON v.id = d.vault_id WHERE r.id = ?")
        .bind(revision_id).fetch_optional(pool).await?.ok_or_else(|| anyhow::anyhow!("vault revision is missing"))?;
    let root: String = row.try_get("canonical_root")?;
    let identity = platform::verified_root(Path::new(&root))?;
    if identity.canonical_root != root
        || identity.device != row.try_get::<i64, _>("root_device")?
        || identity.inode != row.try_get::<i64, _>("root_inode")?
    {
        anyhow::bail!("vault root identity changed")
    }
    let relative: String = row.try_get("relative_path")?;
    let expected_hash: String = row.try_get("sha256")?;
    let file = open_scoped_verified(
        Path::new(&root),
        row.try_get("root_device")?,
        row.try_get("root_inode")?,
        &relative,
    )?;
    if !file.metadata()?.is_file() {
        anyhow::bail!("vault revision is not a regular file")
    }
    if hash_reader(file)? != expected_hash {
        anyhow::bail!("vault revision drifted")
    }
    Ok(Path::new(&root).join(relative))
}

pub(crate) async fn read_revision_at(
    root: &str,
    revision_id: &str,
    pool: &SqlitePool,
) -> anyhow::Result<Vec<u8>> {
    let relative: String =
        sqlx::query_scalar("SELECT relative_path FROM vault_revisions WHERE id = ?")
            .bind(revision_id)
            .fetch_one(pool)
            .await?;
    read_scoped(Path::new(root), &relative)
}

pub(crate) fn publish(
    root: &Path,
    device: i64,
    inode: i64,
    relative: &str,
    content: &[u8],
) -> anyhow::Result<()> {
    let mut reader = std::io::Cursor::new(content);
    publish_reader(
        root,
        device,
        inode,
        relative,
        &mut reader,
        &hash(content),
        content.len() as u64,
    )
}

pub(crate) fn publish_reader<R: Read>(
    root: &Path,
    device: i64,
    inode: i64,
    relative: &str,
    reader: &mut R,
    expected_hash: &str,
    expected_size: u64,
) -> anyhow::Result<()> {
    platform::require_supported()?;
    #[cfg(target_os = "macos")]
    return publish_macos(
        root,
        device,
        inode,
        relative,
        reader,
        expected_hash,
        expected_size,
    );
    #[cfg(not(target_os = "macos"))]
    anyhow::bail!("unsupported_platform")
}

#[cfg(target_os = "macos")]
fn open_import(path: &Path) -> anyhow::Result<File> {
    use std::ffi::CString;
    use std::os::fd::FromRawFd;
    let name = CString::new(path.as_os_str().as_encoded_bytes())?;
    let fd = unsafe {
        nix::libc::open(
            name.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_NONBLOCK
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(target_os = "macos"))]
fn open_import(path: &Path) -> anyhow::Result<File> {
    Ok(File::open(path)?)
}

fn read_scoped(root: &Path, relative: &str) -> anyhow::Result<Vec<u8>> {
    let file = open_scoped(root, relative)?;
    ensure_regular(&file)?;
    let size = file.metadata()?.len();
    read_limited(file, size)
}

fn read_scoped_verified(
    root: &Path,
    device: i64,
    inode: i64,
    relative: &str,
) -> anyhow::Result<Vec<u8>> {
    let file = open_scoped_verified(root, device, inode, relative)?;
    ensure_regular(&file)?;
    let size = file.metadata()?.len();
    read_limited(file, size)
}

fn open_scoped(root: &Path, relative: &str) -> anyhow::Result<File> {
    Ok(crate::evidence::scoped_file::open(root, relative)?)
}

pub(crate) fn open_scoped_verified(
    root: &Path,
    device: i64,
    inode: i64,
    relative: &str,
) -> anyhow::Result<File> {
    Ok(crate::evidence::scoped_file::open_verified(
        root, relative, device, inode,
    )?)
}

fn ensure_regular(file: &File) -> anyhow::Result<()> {
    if !file.metadata()?.is_file() {
        anyhow::bail!("vault imports require a regular non-symlink file")
    }
    Ok(())
}

fn read_limited(file: File, size: u64) -> anyhow::Result<Vec<u8>> {
    if size > MAX_IMPORT_BYTES {
        anyhow::bail!("vault import exceeds 100 MiB")
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_IMPORT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        anyhow::bail!("vault import exceeds 100 MiB")
    }
    Ok(bytes)
}

fn hash_reader(mut file: File) -> anyhow::Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn hash_import(file: &mut File) -> anyhow::Result<(String, u64)> {
    if file.metadata()?.len() > MAX_IMPORT_BYTES {
        anyhow::bail!("vault import exceeds 100 MiB")
    }
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > MAX_IMPORT_BYTES {
            anyhow::bail!("vault import exceeds 100 MiB")
        }
        digest.update(&buffer[..count]);
    }
    Ok((format!("{:x}", digest.finalize()), size))
}

pub(crate) struct VaultRoot {
    pub path: PathBuf,
    pub device: i64,
    pub inode: i64,
}

pub(crate) async fn vault_root(pool: &SqlitePool, vault_id: &str) -> anyhow::Result<VaultRoot> {
    let row: (String, i64, i64) = sqlx::query_as(
        "SELECT canonical_root, root_device, root_inode FROM vaults WHERE id = ? AND enabled = 1",
    )
    .bind(vault_id)
    .fetch_one(pool)
    .await?;
    let root = PathBuf::from(&row.0);
    let identity = platform::verified_root(&root)?;
    if identity.canonical_root != row.0 || identity.device != row.1 || identity.inode != row.2 {
        anyhow::bail!("vault root identity changed")
    }
    Ok(VaultRoot {
        path: root,
        device: row.1,
        inode: row.2,
    })
}

fn relative_path(path: &Path) -> Option<String> {
    let valid = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    valid.then(|| path.to_string_lossy().into_owned())
}

fn safe_name(path: &Path) -> anyhow::Result<String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("vault source name is invalid"))?;
    (!name.is_empty() && !name.contains('/'))
        .then(|| name.to_owned())
        .ok_or_else(|| anyhow::anyhow!("vault source name is invalid"))
}

pub(crate) fn hash(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

#[cfg(target_os = "macos")]
fn publish_macos<R: Read>(
    root: &Path,
    device: i64,
    inode: i64,
    relative: &str,
    reader: &mut R,
    expected_hash: &str,
    expected_size: u64,
) -> anyhow::Result<()> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let parts = normal_parts(relative)?;
    let root_name = CString::new(root.as_os_str().as_encoded_bytes())?;
    let fd = unsafe {
        nix::libc::open(
            root_name.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_DIRECTORY
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut directory = unsafe { File::from_raw_fd(fd) };
    let metadata = directory.metadata()?;
    if metadata.dev() as i64 != device || metadata.ino() as i64 != inode {
        anyhow::bail!("vault root identity changed")
    }
    for part in &parts[..parts.len() - 1] {
        directory = open_or_create_directory(&directory, part)?;
    }
    let name = CString::new(
        parts
            .last()
            .ok_or_else(|| anyhow::anyhow!("vault target is empty"))?
            .as_bytes(),
    )?;
    let temporary = CString::new(format!(
        ".vault-{}-{}.tmp",
        std::process::id(),
        super::catalog::identifier("tmp")?
    ))?;
    let file_fd = unsafe {
        nix::libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            nix::libc::O_WRONLY
                | nix::libc::O_CREAT
                | nix::libc::O_EXCL
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
            0o600,
        )
    };
    if file_fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut file = unsafe { File::from_raw_fd(file_fd) };
    let result = (|| -> anyhow::Result<()> {
        copy_expected(reader, &mut file, expected_hash, expected_size)?;
        file.sync_all()?;
        drop(file);
        if unsafe {
            nix::libc::linkat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                name.as_ptr(),
                0,
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        if unsafe { nix::libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        directory.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            nix::libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0);
        }
    }
    result?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_expected<R: Read>(
    reader: &mut R,
    output: &mut File,
    expected_hash: &str,
    expected_size: u64,
) -> anyhow::Result<()> {
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > expected_size {
            anyhow::bail!("vault import changed during publication")
        }
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    if size != expected_size || format!("{:x}", digest.finalize()) != expected_hash {
        anyhow::bail!("vault import changed during publication")
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn normal_parts(relative: &str) -> anyhow::Result<Vec<std::ffi::OsString>> {
    let parts: Vec<_> = Path::new(relative)
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name.to_os_string()),
            _ => anyhow::bail!("invalid vault relative path"),
        })
        .collect::<anyhow::Result<_>>()?;
    if parts.is_empty() {
        anyhow::bail!("invalid vault relative path")
    }
    Ok(parts)
}

#[cfg(target_os = "macos")]
fn open_or_create_directory(parent: &File, part: &std::ffi::OsStr) -> anyhow::Result<File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(part.as_bytes())?;
    let flags =
        nix::libc::O_RDONLY | nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC;
    let mut fd = unsafe { nix::libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        if unsafe { nix::libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        };
        // Persist the new directory entry before a committed revision can refer to it.
        parent.sync_all()?;
        fd = unsafe { nix::libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    }
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
