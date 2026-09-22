//! Atomic publication under a no-follow directory descriptor (macOS vaults only).
use super::{read_at, VaultRoot};
use std::path::Path;

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        ffi::CString,
        fs::File,
        io::Write,
        os::fd::{AsRawFd, FromRawFd},
        os::unix::fs::MetadataExt,
    };

    fn directory(root: &VaultRoot, path: &str) -> anyhow::Result<(File, CString)> {
        super::super::page_path(path)?;
        let mut parts = path.split('/').collect::<Vec<_>>();
        let name = CString::new(parts.pop().unwrap())?;
        let root_name = CString::new(root.path.as_os_str().as_encoded_bytes())?;
        let flags = nix::libc::O_RDONLY
            | nix::libc::O_DIRECTORY
            | nix::libc::O_NOFOLLOW
            | nix::libc::O_CLOEXEC;
        let fd = unsafe { nix::libc::open(root_name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut dir = unsafe { File::from_raw_fd(fd) };
        let meta = dir.metadata()?;
        if meta.dev() as i64 != root.device || meta.ino() as i64 != root.inode {
            anyhow::bail!("창고 폴더가 변경됐습니다");
        }
        for part in parts {
            let part = CString::new(part)?;
            let fd = unsafe { nix::libc::openat(dir.as_raw_fd(), part.as_ptr(), flags) };
            if fd < 0 {
                anyhow::bail!(
                    "저장할 폴더가 없거나 안전하게 열 수 없습니다: {}",
                    std::io::Error::last_os_error()
                );
            }
            dir = unsafe { File::from_raw_fd(fd) };
        }
        Ok((dir, name))
    }

    fn unchanged(root: &VaultRoot, path: &str, expected: &str) -> anyhow::Result<()> {
        if read_at(root, path)?.sha256 != expected {
            anyhow::bail!("다른 곳에서 문서가 변경됐습니다. 원문을 복사해 보관하거나 편집을 취소한 뒤 새로 고침하세요.");
        }
        Ok(())
    }

    pub fn save(
        root: &VaultRoot,
        path: &str,
        content: &str,
        expected: Option<&str>,
    ) -> anyhow::Result<()> {
        let (dir, name) = directory(root, path)?;
        if let Some(expected) = expected {
            unchanged(root, path, expected)?;
        }
        let temporary = CString::new(format!(
            ".wiki-{}.tmp",
            crate::knowledge::vault::catalog::identifier("write")?
        ))?;
        let fd = unsafe {
            nix::libc::openat(
                dir.as_raw_fd(),
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
            return Err(std::io::Error::last_os_error().into());
        }
        let result = (|| -> anyhow::Result<()> {
            let mut file = unsafe { File::from_raw_fd(fd) };
            if expected.is_some() {
                let original =
                    super::super::open_scoped_verified(&root.path, root.device, root.inode, path)?;
                file.set_permissions(original.metadata()?.permissions())?;
            }
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            if let Some(expected) = expected {
                unchanged(root, path, expected)?;
            }
            let code = unsafe {
                if expected.is_some() {
                    nix::libc::renameat(
                        dir.as_raw_fd(),
                        temporary.as_ptr(),
                        dir.as_raw_fd(),
                        name.as_ptr(),
                    )
                } else {
                    nix::libc::linkat(
                        dir.as_raw_fd(),
                        temporary.as_ptr(),
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        0,
                    )
                }
            };
            if code < 0 {
                anyhow::bail!(
                    "문서를 저장하지 못했습니다 (같은 경로가 있는지 확인하세요): {}",
                    std::io::Error::last_os_error()
                );
            }
            dir.sync_all()?;
            Ok(())
        })();
        unsafe {
            nix::libc::unlinkat(dir.as_raw_fd(), temporary.as_ptr(), 0);
        }
        result
    }

    pub fn trash_with(
        root: &VaultRoot,
        path: &str,
        expected: &str,
        remove: impl FnOnce(&Path) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        let (dir, name) = directory(root, path)?;
        unchanged(root, path, expected)?;
        // Stage via the already-open parent, so OS trash never resolves a mutable
        // vault path. An error restores with linkat(NO REPLACE); recovery never
        // overwrites a newly created file at the original path.
        let stage = std::env::temp_dir().join(crate::knowledge::vault::catalog::identifier(
            "praxis-wiki-trash",
        )?);
        std::fs::DirBuilder::new().mode(0o700).create(&stage)?;
        let staged = stage.join(Path::new(path).file_name().unwrap());
        let staged_name = CString::new(staged.as_os_str().as_encoded_bytes())?;
        let moved = unsafe {
            nix::libc::renameat(
                dir.as_raw_fd(),
                name.as_ptr(),
                nix::libc::AT_FDCWD,
                staged_name.as_ptr(),
            )
        };
        if moved < 0 {
            let error = std::io::Error::last_os_error();
            let _ = std::fs::remove_dir(&stage);
            anyhow::bail!("휴지통 이동을 준비하지 못했습니다. 원본은 그대로 있습니다: {error}");
        }
        let result = (|| -> anyhow::Result<()> {
            let metadata = staged.symlink_metadata()?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > super::super::MAX_PAGE
                || super::super::hash(&std::fs::read(&staged)?) != expected
            {
                anyhow::bail!("삭제 준비 중 문서가 변경됐습니다");
            }
            remove(&staged)
        })();
        if let Err(error) = result {
            let restored = unsafe {
                nix::libc::linkat(
                    nix::libc::AT_FDCWD,
                    staged_name.as_ptr(),
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    0,
                )
            };
            if restored < 0 {
                anyhow::bail!(
                    "휴지통 이동 실패: {error}. 원본 위치와 충돌하여 파일을 보존했습니다: {}",
                    staged.display()
                );
            }
            std::fs::remove_file(&staged)?;
            let _ = std::fs::remove_dir(&stage);
            return Err(error);
        }
        let _ = std::fs::remove_dir(&stage);
        dir.sync_all()?;
        Ok(())
    }
}
#[cfg(unix)]
pub(super) use unix::{save, trash_with};
#[cfg(not(unix))]
pub(super) fn save(_: &VaultRoot, _: &str, _: &str, _: Option<&str>) -> anyhow::Result<()> {
    anyhow::bail!("unsupported_platform")
}
#[cfg(not(unix))]
pub(super) fn trash_with(
    _: &VaultRoot,
    _: &str,
    _: &str,
    _: impl FnOnce(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    anyhow::bail!("unsupported_platform")
}
