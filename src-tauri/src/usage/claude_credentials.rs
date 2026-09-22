//! Claude Code OAuth 자격증명 로딩. 값은 이 모듈 안에서만 보관한다.

use std::path::Path;

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
#[cfg(target_os = "macos")]
const SECURITY_PATH: &str = "/usr/bin/security";

#[derive(PartialEq, Eq)]
pub(crate) enum CredentialError {
    /// 자격증명이 아예 없다 — 로그인한 적이 없거나 로그아웃했다.
    Unauthenticated,
    /// 토큰이 있으나 `expiresAt`이 지났다. CLI가 실행되면 refresh token으로 스스로 갱신하므로
    /// 로그아웃이 아니다 — 새 값을 못 받았을 뿐이다.
    Expired,
    Error,
}

pub(crate) struct ClaudeCredentials {
    access_token: String,
    plan: Option<String>,
}

impl ClaudeCredentials {
    pub(crate) fn into_parts(self) -> (String, Option<String>) {
        (self.access_token, self.plan)
    }
}

pub(crate) fn parse_credentials_json(
    raw: &str,
    now: i64,
) -> Result<ClaudeCredentials, CredentialError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| CredentialError::Error)?;
    let oauth = value
        .get("claudeAiOauth")
        .and_then(serde_json::Value::as_object)
        .ok_or(CredentialError::Error)?;
    let access_token = oauth
        .get("accessToken")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or(CredentialError::Error)?;
    if let Some(expires_at) = oauth.get("expiresAt") {
        let expires_at = expires_at.as_i64().ok_or(CredentialError::Error)?;
        if expires_at / 1000 <= now {
            return Err(CredentialError::Expired);
        }
    }
    let plan = oauth.get("subscriptionType").map_or(Ok(None), |value| {
        value
            .as_str()
            .map(str::to_string)
            .map(Some)
            .ok_or(CredentialError::Error)
    })?;
    Ok(ClaudeCredentials {
        access_token: access_token.to_string(),
        plan,
    })
}

pub(crate) async fn load_credentials(
    home: &Path,
    now: i64,
) -> Result<ClaudeCredentials, CredentialError> {
    #[cfg(target_os = "macos")]
    {
        let keychain = tokio::task::spawn_blocking(load_from_keychain)
            .await
            .map_err(|_| CredentialError::Error)?;
        match keychain? {
            Some(raw) => parse_credentials_json(&raw, now),
            None => load_legacy_file(home, now),
        }
    }
    #[cfg(not(target_os = "macos"))]
    load_legacy_file(home, now)
}

fn load_legacy_file(home: &Path, now: i64) -> Result<ClaudeCredentials, CredentialError> {
    let path = home.join(".claude").join(".credentials.json");
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CredentialError::Unauthenticated)
        }
        Err(_) => return Err(CredentialError::Error),
    };
    parse_credentials_json(&raw, now)
}

#[cfg(target_os = "macos")]
fn load_from_keychain() -> Result<Option<String>, CredentialError> {
    use std::collections::HashMap;

    use keyring_core::api::CredentialStoreApi;

    let store =
        apple_native_keyring_store::keychain::Store::new().map_err(|_| CredentialError::Error)?;
    let spec = HashMap::from([("service", KEYCHAIN_SERVICE)]);
    let entries = store.search(&spec).map_err(|_| CredentialError::Error)?;
    match entries.len() {
        0 => Ok(None),
        1 => {
            let entry = entries.into_iter().next().expect("one Keychain match");
            let (service, account) = entry.get_specifiers().ok_or(CredentialError::Error)?;
            if service != KEYCHAIN_SERVICE {
                return Err(CredentialError::Error);
            }
            read_security_password(
                Path::new(SECURITY_PATH),
                &account,
                std::time::Duration::from_secs(5),
            )
            .map(Some)
        }
        _ => Err(CredentialError::Error),
    }
}

#[cfg(target_os = "macos")]
pub(super) fn read_security_password(
    command: &Path,
    account: &str,
    timeout: std::time::Duration,
) -> Result<String, CredentialError> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new(command)
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            account,
            "-w",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CredentialError::Error)?;
    wait_for_child(&mut child, timeout)?;
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or(CredentialError::Error)?
        .read_to_end(&mut output)
        .map_err(|_| CredentialError::Error)?;
    let password = String::from_utf8(output).map_err(|_| CredentialError::Error)?;
    let password = password.trim_end_matches(['\r', '\n']);
    if password.is_empty() {
        return Err(CredentialError::Error);
    }
    Ok(password.to_string())
}

#[cfg(target_os = "macos")]
fn wait_for_child(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> Result<(), CredentialError> {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(CredentialError::Error),
            Err(_) => {
                reap_child(child);
                return Err(CredentialError::Error);
            }
            Ok(None) if std::time::Instant::now() >= deadline => {
                reap_child(child);
                return Err(CredentialError::Error);
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

#[cfg(target_os = "macos")]
fn reap_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}
