//! Race-resistant, non-blocking reads rooted at a canonical repository directory.

use std::fs::File;
use std::path::Path;

pub(crate) struct DirectoryEntries {
    pub names: Vec<std::ffi::OsString>,
    pub limited: bool,
    pub inspected_entries: usize,
}

#[cfg(unix)]
pub(crate) fn open(root: &Path, relative_path: &str) -> std::io::Result<File> {
    open_with_identity(root, relative_path, None)
}

#[cfg(unix)]
pub(crate) fn open_verified(
    root: &Path,
    relative_path: &str,
    device: i64,
    inode: i64,
) -> std::io::Result<File> {
    open_with_identity(root, relative_path, Some((device, inode)))
}

#[cfg(unix)]
fn open_with_identity(
    root: &Path,
    relative_path: &str,
    expected: Option<(i64, i64)>,
) -> std::io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;

    let components = normal_components(relative_path)?;
    let root_name = cstring(root.as_os_str())?;
    let root_fd = unsafe {
        nix::libc::open(
            root_name.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_DIRECTORY
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if root_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut directory = unsafe { File::from_raw_fd(root_fd) };
    if let Some((device, inode)) = expected {
        let metadata = directory.metadata()?;
        if metadata.dev() as i64 != device || metadata.ino() as i64 != inode {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "scoped root identity changed",
            ));
        }
    }
    for component in &components[..components.len() - 1] {
        let name = cstring(component)?;
        let fd = unsafe {
            nix::libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                nix::libc::O_RDONLY
                    | nix::libc::O_DIRECTORY
                    | nix::libc::O_NOFOLLOW
                    | nix::libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        directory = unsafe { File::from_raw_fd(fd) };
    }
    open_leaf(&directory, components.last().unwrap())
}

#[cfg(unix)]
pub(crate) fn list(
    root: &Path,
    relative_path: &str,
    limit: usize,
) -> std::io::Result<DirectoryEntries> {
    directory::list(root, relative_path, limit)
}

#[cfg(unix)]
fn open_leaf(directory: &File, name: &std::ffi::OsStr) -> std::io::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let name = cstring(name)?;
    let fd = unsafe {
        nix::libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_NONBLOCK
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn normal_components(relative_path: &str) -> std::io::Result<Vec<&std::ffi::OsStr>> {
    use std::path::Component;

    let mut names = Vec::new();
    for component in Path::new(relative_path).components() {
        match component {
            Component::Normal(name) => names.push(name),
            Component::CurDir => {}
            _ => return Err(invalid_path()),
        }
    }
    if names.is_empty() {
        return Err(invalid_path());
    }
    Ok(names)
}

#[cfg(unix)]
fn normal_directory_components(relative_path: &str) -> std::io::Result<Vec<&std::ffi::OsStr>> {
    if relative_path.is_empty() || relative_path == "." {
        return Ok(Vec::new());
    }
    normal_components(relative_path)
}

#[cfg(unix)]
#[path = "scoped_file_list.rs"]
mod directory;

#[cfg(unix)]
fn cstring(value: &std::ffi::OsStr) -> std::io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(value.as_bytes()).map_err(|_| invalid_path())
}

#[cfg(unix)]
fn invalid_path() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid scoped file path")
}

#[cfg(windows)]
pub(crate) fn open(_root: &Path, _relative_path: &str) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "secure directory-relative evidence reads are not yet supported on Windows",
    ))
}

#[cfg(windows)]
pub(crate) fn open_verified(
    _root: &Path,
    _relative_path: &str,
    _device: i64,
    _inode: i64,
) -> std::io::Result<File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "secure directory-relative evidence reads are not yet supported on Windows",
    ))
}

#[cfg(windows)]
pub(crate) fn list(
    _root: &Path,
    _relative_path: &str,
    _limit: usize,
) -> std::io::Result<DirectoryEntries> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "secure directory-relative evidence reads are not yet supported on Windows",
    ))
}

#[cfg(all(test, unix))]
#[path = "scoped_file_tests.rs"]
mod tests;
