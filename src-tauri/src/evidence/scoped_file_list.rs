use super::*;
use std::ffi::CStr;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::ffi::OsStrExt;

pub(super) fn list(
    root: &Path,
    relative_path: &str,
    limit: usize,
) -> std::io::Result<DirectoryEntries> {
    let directory = open_directory(root, relative_path)?;
    let fd = directory.into_raw_fd();
    let stream = unsafe { nix::libc::fdopendir(fd) };
    if stream.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe { nix::libc::close(fd) };
        return Err(error);
    }
    let mut entries = DirectoryEntries {
        names: Vec::with_capacity(limit),
        limited: false,
        inspected_entries: 0,
    };
    loop {
        nix::errno::Errno::clear();
        let entry = unsafe { nix::libc::readdir(stream) };
        if entry.is_null() {
            let errno = nix::errno::Errno::last();
            if errno != nix::errno::Errno::UnknownErrno {
                unsafe { nix::libc::closedir(stream) };
                return Err(std::io::Error::from_raw_os_error(errno as i32));
            }
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        entries.inspected_entries += 1;
        if entries.inspected_entries > limit {
            entries.limited = true;
            break;
        }
        entries
            .names
            .push(std::ffi::OsStr::from_bytes(name).to_os_string());
    }
    unsafe { nix::libc::closedir(stream) };
    Ok(entries)
}

fn open_directory(root: &Path, relative_path: &str) -> std::io::Result<File> {
    let components = normal_directory_components(relative_path)?;
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
    for component in components {
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
    Ok(directory)
}
