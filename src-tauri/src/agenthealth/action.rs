//! 인증·업데이트 액션의 명령 조립.
//!
//! 명령을 하드코딩하지 않는다 — 같은 CLI라도 설치 방식에 따라 업데이트 경로가 다르다
//! (claude=native `claude update`, codex=npm `npm i -g`). 판별에 실패하면 **명령을 만들지
//! 않는다.** 추측해서 엉뚱한 패키지 매니저를 돌리는 것보다 못 하는 편이 낫다.

use serde::Deserialize;

use super::detect::InstallMethod;

/// 사용자가 누를 수 있는 액션.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Login,
    Logout,
    Update,
    Doctor,
    /// 워크스페이스에 묶이지 않은 자유 셸 — 벤더와 무관하다.
    Scratch,
}

impl ActionKind {
    /// PTY 세션 맵의 키 접두사.
    pub fn key(self) -> &'static str {
        match self {
            ActionKind::Login => "login",
            ActionKind::Logout => "logout",
            ActionKind::Update => "update",
            ActionKind::Doctor => "doctor",
            ActionKind::Scratch => "scratch",
        }
    }

    /// 이 액션이 바이너리를 갈아치우는가 — 실행 중인 작업과 함께 둘 수 없다.
    pub fn replaces_binary(self) -> bool {
        self == ActionKind::Update
    }
}

/// 실행할 명령 — `bin`은 아직 PATH 해석 전 이름이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCommand {
    pub bin: String,
    pub args: Vec<String>,
}

fn cmd(bin: &str, args: &[&str]) -> ActionCommand {
    ActionCommand {
        bin: bin.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
    }
}

/// 액션 → 명령. `Scratch`는 셸이라 여기서 다루지 않는다(호출부가 `default_shell`을 쓴다).
///
/// `package`는 npm 전역 설치일 때만 쓰인다. 없으면 npm 업데이트를 만들 수 없다.
pub fn command_for(
    kind: ActionKind,
    vendor: &str,
    method: InstallMethod,
    package: Option<&str>,
) -> Result<ActionCommand, String> {
    match (kind, vendor) {
        (ActionKind::Scratch, _) => Err("Scratch는 셸이라 명령이 없습니다".into()),

        // 로그인 — Codex는 `--device-auth`로 고정한다. 브라우저 콜백 서버를 띄우지 않아
        // 앱 안 PTY에서도, SSH 너머 원격 Runner에서도 같은 코드가 통한다.
        (ActionKind::Login, "claude") => Ok(cmd("claude", &["auth", "login"])),
        (ActionKind::Login, "codex") => Ok(cmd("codex", &["login", "--device-auth"])),
        (ActionKind::Logout, "claude") => Ok(cmd("claude", &["auth", "logout"])),
        (ActionKind::Logout, "codex") => Ok(cmd("codex", &["logout"])),

        (ActionKind::Doctor, "claude") => Ok(cmd("claude", &["doctor"])),
        (ActionKind::Doctor, vendor) => Err(format!("{vendor}에는 진단 명령이 없습니다")),

        (ActionKind::Update, vendor) => update_command(vendor, method, package),

        (_, vendor) => Err(format!("알 수 없는 벤더입니다: {vendor}")),
    }
}

fn update_command(
    vendor: &str,
    method: InstallMethod,
    package: Option<&str>,
) -> Result<ActionCommand, String> {
    match method {
        // claude만 자체 업데이터를 갖는다. codex는 native 설치를 쓰지 않는다.
        InstallMethod::Native if vendor == "claude" => Ok(cmd("claude", &["update"])),
        InstallMethod::Native => Err(format!(
            "{vendor}는 자체 업데이트 명령이 없습니다 — 설치한 방법으로 갱신하세요"
        )),
        InstallMethod::Npm => {
            let package = package.ok_or_else(|| format!("{vendor}의 npm 패키지를 모릅니다"))?;
            Ok(cmd("npm", &["install", "-g", package]))
        }
        InstallMethod::Homebrew => Ok(cmd("brew", &["upgrade", vendor])),
        InstallMethod::Unknown => Err(
            "설치 방식을 알 수 없어 업데이트 명령을 만들지 않았습니다 — 경로를 확인하세요".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_uses_device_auth_for_codex_only() {
        // codex는 브라우저 콜백 서버 없이 끝나야 한다 — 원격 Runner에서도 같은 코드가 돈다.
        assert_eq!(
            command_for(ActionKind::Login, "codex", InstallMethod::Npm, None).unwrap(),
            cmd("codex", &["login", "--device-auth"])
        );
        assert_eq!(
            command_for(ActionKind::Login, "claude", InstallMethod::Native, None).unwrap(),
            cmd("claude", &["auth", "login"])
        );
    }

    #[test]
    fn logout_is_asymmetric_between_vendors() {
        // claude는 `auth logout`, codex는 최상위 `logout` — 대칭이라고 가정하면 틀린다.
        assert_eq!(
            command_for(ActionKind::Logout, "claude", InstallMethod::Native, None).unwrap(),
            cmd("claude", &["auth", "logout"])
        );
        assert_eq!(
            command_for(ActionKind::Logout, "codex", InstallMethod::Npm, None).unwrap(),
            cmd("codex", &["logout"])
        );
    }

    #[test]
    fn update_follows_install_method_not_vendor() {
        // 이 개발기의 실제 배치: claude=native, codex=npm.
        assert_eq!(
            command_for(ActionKind::Update, "claude", InstallMethod::Native, None).unwrap(),
            cmd("claude", &["update"])
        );
        assert_eq!(
            command_for(
                ActionKind::Update,
                "codex",
                InstallMethod::Npm,
                Some("@openai/codex")
            )
            .unwrap(),
            cmd("npm", &["install", "-g", "@openai/codex"])
        );
    }

    #[test]
    fn npm_installed_claude_does_not_get_the_native_updater() {
        // 같은 벤더라도 설치 방식이 다르면 명령이 다르다. 벤더로 분기하면 여기서 틀린다.
        assert_eq!(
            command_for(
                ActionKind::Update,
                "claude",
                InstallMethod::Npm,
                Some("@anthropic-ai/claude-code")
            )
            .unwrap(),
            cmd("npm", &["install", "-g", "@anthropic-ai/claude-code"])
        );
    }

    #[test]
    fn unknown_install_method_makes_no_command() {
        // 추측해서 엉뚱한 패키지 매니저를 돌리느니 아무것도 하지 않는다.
        assert!(command_for(
            ActionKind::Update,
            "codex",
            InstallMethod::Unknown,
            Some("@openai/codex")
        )
        .is_err());
    }

    #[test]
    fn npm_update_without_a_package_is_refused() {
        assert!(command_for(ActionKind::Update, "codex", InstallMethod::Npm, None).is_err());
    }

    #[test]
    fn codex_has_no_doctor() {
        assert!(command_for(ActionKind::Doctor, "claude", InstallMethod::Native, None).is_ok());
        assert!(command_for(ActionKind::Doctor, "codex", InstallMethod::Npm, None).is_err());
    }

    #[test]
    fn only_update_replaces_the_binary() {
        // 이 술어가 곧 실행 중 작업 가드의 조건이다 — 로그인까지 막으면 못 고치는 상황이 생긴다.
        assert!(ActionKind::Update.replaces_binary());
        assert!(!ActionKind::Login.replaces_binary());
        assert!(!ActionKind::Logout.replaces_binary());
        assert!(!ActionKind::Scratch.replaces_binary());
    }

    #[test]
    fn session_keys_are_distinct_per_action() {
        let keys = [
            ActionKind::Login.key(),
            ActionKind::Logout.key(),
            ActionKind::Update.key(),
            ActionKind::Doctor.key(),
            ActionKind::Scratch.key(),
        ];
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }
}
