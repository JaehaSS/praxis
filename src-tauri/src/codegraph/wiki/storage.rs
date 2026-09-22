#[cfg(unix)]
use std::ffi::{CString, OsStr};
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::{Read, Write};
use std::path::{Component, Path};

use super::model::MAX_PREIMAGE_BYTES;
use super::render;

pub(super) const ROOT: &str = "docs/codebase";

pub(super) struct Preimage {
    pub relative: String,
    pub bytes: Option<Vec<u8>>,
}

pub(super) fn page_path(source: &str) -> String {
    format!("{ROOT}/modules/{source}.md")
}

pub(super) fn read(root: &Path, relative: &str) -> anyhow::Result<Option<String>> {
    #[cfg(not(unix))]
    {
        let _ = (root, relative);
        anyhow::bail!("code Wiki secure file access is supported only on Unix");
    }
    #[cfg(unix)]
    {
        match crate::evidence::scoped_file::open(root, relative) {
            Ok(file) => {
                let metadata = file.metadata()?;
                if !metadata.is_file() || metadata.len() > super::model::MAX_INDEX_BYTES as u64 {
                    anyhow::bail!("code Wiki output is not a readable regular file");
                }
                let mut text = String::new();
                file.take(super::model::MAX_INDEX_BYTES as u64 + 1)
                    .read_to_string(&mut text)?;
                if text.len() > super::model::MAX_INDEX_BYTES {
                    anyhow::bail!("code Wiki output exceeds its read limit");
                }
                Ok(Some(text))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

pub(super) fn preflight(root: &Path, outputs: &[(&str, &str)]) -> anyhow::Result<Vec<Preimage>> {
    ensure_unix()?;
    let mut preimages = Vec::with_capacity(outputs.len());
    let mut bytes = 0usize;
    for (path, expected_kind) in outputs {
        let text = read(root, path)?;
        if let Some(text) = &text {
            let Some((meta, _)) = render::parse_document(text) else {
                anyhow::bail!("refusing to overwrite manual or modified code Wiki page: {path}");
            };
            if meta.kind != *expected_kind {
                anyhow::bail!(
                    "refusing to overwrite code Wiki path with a different page kind: {path}"
                );
            }
        }
        bytes = bytes
            .checked_add(text.as_ref().map_or(0, String::len))
            .ok_or_else(|| anyhow::anyhow!("code Wiki preimage size overflow"))?;
        if bytes > MAX_PREIMAGE_BYTES {
            anyhow::bail!(
                "code Wiki preimages exceed the {MAX_PREIMAGE_BYTES} byte aggregate limit"
            );
        }
        preimages.push(Preimage {
            relative: (*path).to_owned(),
            bytes: text.map(String::into_bytes),
        });
    }
    Ok(preimages)
}

pub(super) fn write(root: &Path, preimage: &Preimage, content: &str) -> anyhow::Result<()> {
    ensure_unix()?;
    #[cfg(unix)]
    {
        write_unix(root, preimage, content).map_err(Into::into)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, preimage, content);
        unreachable!()
    }
}

pub(super) fn lock(root: &Path) -> anyhow::Result<OutputLock> {
    ensure_unix()?;
    #[cfg(unix)]
    {
        let path = root.join(ROOT).join(".praxis-codewiki.lock");
        OutputLock::acquire(root).map_err(|error| anyhow::anyhow!("code Wiki output lock exists or cannot be created at {}: {error}; remove it only after confirming no generation is running", path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        unreachable!()
    }
}

fn ensure_unix() -> anyhow::Result<()> {
    #[cfg(not(unix))]
    anyhow::bail!("code Wiki generation is unsupported on this platform because secure descriptor-relative writes are unavailable");
    #[cfg(unix)]
    Ok(())
}

#[cfg(unix)]
pub(super) struct OutputLock {
    directory: File,
}

#[cfg(unix)]
impl OutputLock {
    fn acquire(root: &Path) -> std::io::Result<Self> {
        let (directory, _) = parent_dir(root, "docs/codebase/.praxis-codewiki.lock", true)?;
        let name = cstring(OsStr::new(".praxis-codewiki.lock"))?;
        let fd = unsafe {
            nix::libc::openat(
                directory_fd(&directory),
                name.as_ptr(),
                nix::libc::O_WRONLY
                    | nix::libc::O_CREAT
                    | nix::libc::O_EXCL
                    | nix::libc::O_NOFOLLOW
                    | nix::libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        unsafe {
            nix::libc::close(fd);
        }
        Ok(Self { directory })
    }
}

#[cfg(unix)]
impl Drop for OutputLock {
    fn drop(&mut self) {
        let name = CString::new(".praxis-codewiki.lock").unwrap();
        unsafe {
            nix::libc::unlinkat(directory_fd(&self.directory), name.as_ptr(), 0);
        }
    }
}

#[cfg(unix)]
fn write_unix(root: &Path, preimage: &Preimage, content: &str) -> std::io::Result<()> {
    let (directory, name) = parent_dir(root, &preimage.relative, true)?;
    let name = cstring(name)?;
    let temporary = CString::new(format!(
        ".praxis-codewiki-{}-{}.tmp",
        std::process::id(),
        super::TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
    .unwrap();
    let fd = unsafe {
        nix::libc::openat(
            directory_fd(&directory),
            temporary.as_ptr(),
            nix::libc::O_WRONLY
                | nix::libc::O_CREAT
                | nix::libc::O_EXCL
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = (|| {
        let mut file: File = unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) };
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if read_leaf(&directory, &name)? != preimage.bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "code Wiki page changed during generation",
            ));
        }
        if unsafe {
            nix::libc::renameat(
                directory_fd(&directory),
                temporary.as_ptr(),
                directory_fd(&directory),
                name.as_ptr(),
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            nix::libc::unlinkat(directory_fd(&directory), temporary.as_ptr(), 0);
        }
    }
    result
}

#[cfg(unix)]
fn parent_dir<'a>(
    root: &Path,
    relative: &'a str,
    create: bool,
) -> std::io::Result<(File, &'a OsStr)> {
    use std::os::fd::FromRawFd;
    let components = components(relative)?;
    let root_name = cstring(root.as_os_str())?;
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
        return Err(std::io::Error::last_os_error());
    }
    let mut directory = unsafe { File::from_raw_fd(fd) };
    for component in &components[..components.len() - 1] {
        let name = cstring(component)?;
        if create {
            unsafe {
                nix::libc::mkdirat(directory_fd(&directory), name.as_ptr(), 0o700);
            }
        }
        let next = unsafe {
            nix::libc::openat(
                directory_fd(&directory),
                name.as_ptr(),
                nix::libc::O_RDONLY
                    | nix::libc::O_DIRECTORY
                    | nix::libc::O_NOFOLLOW
                    | nix::libc::O_CLOEXEC,
            )
        };
        if next < 0 {
            return Err(std::io::Error::last_os_error());
        }
        directory = unsafe { File::from_raw_fd(next) };
    }
    Ok((directory, components.last().unwrap()))
}

#[cfg(unix)]
fn read_leaf(directory: &File, name: &CString) -> std::io::Result<Option<Vec<u8>>> {
    use std::os::fd::FromRawFd;
    let fd = unsafe {
        nix::libc::openat(
            directory_fd(directory),
            name.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_NONBLOCK
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > super::model::MAX_INDEX_BYTES as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "code Wiki output is not a regular file",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize + 1);
    file.take(super::model::MAX_INDEX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > super::model::MAX_INDEX_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "code Wiki output exceeds its read limit",
        ));
    }
    Ok(Some(bytes))
}

#[cfg(unix)]
fn components(relative: &str) -> std::io::Result<Vec<&OsStr>> {
    let values: Vec<_> = Path::new(relative)
        .components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid code Wiki path",
            )),
        })
        .collect::<Result<_, _>>()?;
    if values.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid code Wiki path",
        ));
    }
    Ok(values)
}

#[cfg(unix)]
fn cstring(value: &OsStr) -> std::io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(value.as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid code Wiki path")
    })
}
#[cfg(unix)]
fn directory_fd(file: &File) -> std::os::fd::RawFd {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}
