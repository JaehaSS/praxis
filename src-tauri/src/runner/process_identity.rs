//! OS-backed process birth identity used to avoid killing a reused PGID after restart.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessTerminationOutcome {
    Absent,
    Terminated,
    IdentityMismatch,
}

pub(crate) fn observe(pid: u32) -> anyhow::Result<Option<String>> {
    if pid == 0 {
        anyhow::bail!("process identity requires a non-zero pid");
    }
    platform_token(pid).map(|token| token.map(hash_token))
}

pub(crate) fn observe_group_leader(pid: u32) -> anyhow::Result<Option<String>> {
    let Some(identity) = observe(pid)? else {
        return Ok(None);
    };
    ensure_process_group_leader(pid)?;
    Ok(Some(identity))
}

pub(crate) async fn terminate_if_matches(
    stored_pgid: i64,
    expected: &str,
) -> anyhow::Result<ProcessTerminationOutcome> {
    let pid = checked_process_group_id(stored_pgid)?;
    let Some(current) = observe(pid)? else {
        ensure_no_unidentified_group(crate::verify::process_group_alive_checked(pid)?)?;
        return Ok(ProcessTerminationOutcome::Absent);
    };
    if current != expected {
        return Ok(ProcessTerminationOutcome::IdentityMismatch);
    }
    ensure_process_group_leader(pid)?;
    let Some(confirmed) = observe(pid)? else {
        ensure_no_unidentified_group(crate::verify::process_group_alive_checked(pid)?)?;
        return Ok(ProcessTerminationOutcome::Absent);
    };
    if confirmed != expected {
        return Ok(ProcessTerminationOutcome::IdentityMismatch);
    }
    crate::verify::kill_group_checked(pid)?;
    for _ in 0..20 {
        // kill 직후 확인이라 좀비도 종료로 센다(macOS는 EPERM, 리눅스는 /proc 상태 Z) —
        // 자세한 이유는 verify::process_group_terminated_checked 주석 참고.
        if crate::verify::process_group_terminated_checked(pid)? {
            return Ok(ProcessTerminationOutcome::Terminated);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    anyhow::bail!("verified process group did not terminate")
}

pub(super) fn checked_process_group_id(stored_pgid: i64) -> anyhow::Result<u32> {
    if !(1..=i64::from(i32::MAX)).contains(&stored_pgid) {
        anyhow::bail!("stored process group id is outside the signal-safe range");
    }
    Ok(stored_pgid as u32)
}

fn ensure_process_group_leader(pid: u32) -> anyhow::Result<()> {
    match process_group_id(pid)? {
        Some(pgid) if pgid == pid => Ok(()),
        Some(_) => anyhow::bail!("review process is not its process group leader"),
        None => anyhow::bail!("review process exited before process group verification"),
    }
}

#[cfg(unix)]
fn process_group_id(pid: u32) -> anyhow::Result<Option<u32>> {
    let pid = i32::try_from(pid).map_err(|_| anyhow::anyhow!("process id exceeds pid_t range"))?;
    let pgid = unsafe { nix::libc::getpgid(pid) };
    if pgid >= 0 {
        return Ok(Some(pgid as u32));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(nix::libc::ESRCH) {
        return Ok(None);
    }
    Err(error.into())
}

#[cfg(not(unix))]
fn process_group_id(_pid: u32) -> anyhow::Result<Option<u32>> {
    anyhow::bail!("secure process group identity is unsupported on this platform")
}

fn ensure_no_unidentified_group(group_alive: bool) -> anyhow::Result<()> {
    if group_alive {
        anyhow::bail!("process leader exited while an unverifiable process group remains");
    }
    Ok(())
}

fn hash_token(token: String) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

// libc 0.2.183+에서 kinfo_proc(sysctl KERN_PROC) 타입이 제거되어 proc_pidinfo로 대체.
// 토큰 원문 포맷은 동일(macos:초:마이크로초)하나 출처 API가 달라 기존 저장 토큰과
// 불일치할 수 있다 — 불일치 시 terminate_if_matches는 종료를 건너뛰므로(fail-safe) 안전.
#[cfg(target_os = "macos")]
fn platform_token(pid: u32) -> anyhow::Result<Option<String>> {
    let mut info = std::mem::MaybeUninit::<nix::libc::proc_bsdinfo>::zeroed();
    let size = std::mem::size_of::<nix::libc::proc_bsdinfo>() as libc_c_int;
    let result = unsafe {
        nix::libc::proc_pidinfo(
            pid as libc_c_int,
            nix::libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if result <= 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(nix::libc::ESRCH) {
            return Ok(None);
        }
        return Err(error.into());
    }
    if result < size {
        anyhow::bail!("proc_pidinfo returned truncated bsdinfo");
    }
    let info = unsafe { info.assume_init() };
    Ok(Some(format!(
        "macos:{}:{}",
        info.pbi_start_tvsec, info.pbi_start_tvusec
    )))
}

#[cfg(target_os = "macos")]
use std::os::raw::c_int as libc_c_int;

#[cfg(target_os = "linux")]
fn platform_token(pid: u32) -> anyhow::Result<Option<String>> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let fields = stat
        .rsplit_once(')')
        .ok_or_else(|| anyhow::anyhow!("invalid proc stat"))?
        .1;
    let start_ticks = fields
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| anyhow::anyhow!("proc stat has no start time"))?;
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    Ok(Some(format!("linux:{}:{start_ticks}", boot_id.trim())))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_token(_pid: u32) -> anyhow::Result<Option<String>> {
    anyhow::bail!("secure process birth identity is unsupported on this platform")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn leaderless_live_group_requires_quarantine() {
        assert!(ensure_no_unidentified_group(true).is_err());
        assert!(ensure_no_unidentified_group(false).is_ok());
    }

    #[test]
    fn stored_process_group_ids_never_wrap_before_signaling() {
        assert_eq!(checked_process_group_id(1).unwrap(), 1);
        assert_eq!(
            checked_process_group_id(i64::from(i32::MAX)).unwrap(),
            i32::MAX as u32
        );
        assert!(checked_process_group_id(0).is_err());
        assert!(checked_process_group_id(-1).is_err());
        assert!(checked_process_group_id(i64::from(i32::MAX) + 1).is_err());
    }

    #[tokio::test]
    async fn mismatched_birth_identity_is_never_killed_but_exact_identity_is() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("5").process_group(0);
        let mut child = command.spawn().unwrap();
        let pid = child.id();
        let identity = observe(pid).unwrap().unwrap();
        assert_eq!(observe(pid).unwrap().as_deref(), Some(identity.as_str()));

        assert_eq!(
            terminate_if_matches(i64::from(pid), &"0".repeat(64))
                .await
                .unwrap(),
            ProcessTerminationOutcome::IdentityMismatch
        );
        assert!(crate::verify::process_group_alive(pid));
        assert_eq!(
            terminate_if_matches(i64::from(pid), &identity)
                .await
                .unwrap(),
            ProcessTerminationOutcome::Terminated
        );
        let _ = child.wait();
        assert!(!crate::verify::process_group_alive(pid));
    }
}
