//! Claude Code statusline 브리지 — 공식 statusline JSON의 `rate_limits`를 파일로 남긴다.
//!
//! Claude Code는 잔량을 로컬 어디에도 쓰지 않고, statusLine 명령의 stdin JSON으로만 준다
//! (Pro/Max 구독 + 세션 첫 응답 이후). 그래서 사용자의 `~/.claude/settings.json`에 얇은
//! 래퍼를 끼워 값을 `praxis-usage.json`으로 흘린다.
//!
//! 사용자 설정을 건드리므로 **명시적 설치 액션**으로만 동작하고, 이미 statusLine이 있으면
//! 그 명령을 inner로 보존해 브리지가 대신 실행한다(HUD를 빼앗지 않는다).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// 브리지 설치 상태 — 팝오버 버튼 문구를 정하는 데 쓴다.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub installed: bool,
    /// 브리지가 대신 실행해 주는 기존 statusLine 명령(있을 때만).
    pub wrapped: Option<String>,
    /// 설치되지 않았고, 자리에 다른 statusLine이 있을 때 그 명령.
    pub foreign: Option<String>,
}

fn script_path(home: &Path) -> PathBuf {
    home.join(".claude").join("praxis-statusline.mjs")
}

/// 브리지가 감싼 원래 명령을 적어 두는 파일.
fn inner_path(home: &Path) -> PathBuf {
    home.join(".claude").join("praxis-statusline-inner.txt")
}

fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

/// statusLine.command 문자열(설정이 없거나 형태가 다르면 None).
fn current_command(settings: &serde_json::Value) -> Option<String> {
    settings
        .get("statusLine")?
        .get("command")?
        .as_str()
        .map(str::to_string)
}

fn is_ours(cmd: &str) -> bool {
    cmd.contains("praxis-statusline.mjs")
}

fn read_settings(home: &Path) -> serde_json::Value {
    std::fs::read_to_string(settings_path(home))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// 현재 설치 상태.
pub fn status(home: &Path) -> BridgeStatus {
    let settings = read_settings(home);
    let cmd = current_command(&settings);
    let installed = cmd.as_deref().map(is_ours).unwrap_or(false) && script_path(home).is_file();
    let wrapped = if installed {
        std::fs::read_to_string(inner_path(home))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    BridgeStatus {
        installed,
        wrapped,
        foreign: cmd.filter(|c| !is_ours(c)).filter(|_| !installed),
    }
}

/// 브리지 스크립트 본문. inner 명령이 있으면 그대로 실행해 출력을 통과시킨다.
fn script_body() -> &'static str {
    r#"#!/usr/bin/env node
// praxis 사용량 브리지 (자동 생성) — Claude Code statusline JSON의 rate_limits를
// ~/.claude/praxis-usage.json 으로 덤프하고, 감싼 원래 statusLine 명령이 있으면 실행한다.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const home = homedir();
let raw = "";
try {
  raw = readFileSync(0, "utf8");
} catch {}

try {
  const data = JSON.parse(raw);
  if (data && data.rate_limits) {
    writeFileSync(
      join(home, ".claude", "praxis-usage.json"),
      JSON.stringify({
        updated_at: Math.floor(Date.now() / 1000),
        plan: data.plan || null,
        rate_limits: data.rate_limits,
      })
    );
  }
} catch {}

// 감싼 명령이 있으면 같은 stdin으로 실행해 그 출력을 그대로 statusline에 넘긴다.
const innerFile = join(home, ".claude", "praxis-statusline-inner.txt");
if (existsSync(innerFile)) {
  const inner = readFileSync(innerFile, "utf8").trim();
  if (inner) {
    const r = spawnSync(inner, { shell: true, input: raw, encoding: "utf8" });
    process.stdout.write(r.stdout || "");
    process.exit(0);
  }
}
"#
}

/// 브리지 설치 — 스크립트를 쓰고 settings.json의 statusLine을 브리지로 바꾼다.
/// 기존 명령은 inner로 보존한다. 이미 설치돼 있으면 스크립트만 최신화한다.
pub fn install(home: &Path) -> Result<BridgeStatus, String> {
    let dir = home.join(".claude");
    std::fs::create_dir_all(&dir).map_err(|e| format!("~/.claude 생성 실패: {e}"))?;
    std::fs::write(script_path(home), script_body())
        .map_err(|e| format!("브리지 스크립트 저장 실패: {e}"))?;

    let mut settings = read_settings(home);
    let existing = current_command(&settings);
    // 이미 우리 것이면 inner를 덮어쓰지 않는다(재설치로 원래 명령을 잃지 않도록).
    if let Some(prev) = existing.filter(|c| !is_ours(c)) {
        std::fs::write(inner_path(home), prev)
            .map_err(|e| format!("기존 statusLine 보존 실패: {e}"))?;
    }

    let cmd = format!("node \"{}\"", script_path(home).display());
    let obj = settings
        .as_object_mut()
        .ok_or_else(|| "settings.json 형식이 객체가 아닙니다".to_string())?;
    let line = obj
        .entry("statusLine")
        .or_insert_with(|| serde_json::json!({}));
    if !line.is_object() {
        *line = serde_json::json!({});
    }
    let line = line.as_object_mut().expect("직전에 객체로 맞췄다");
    line.insert("type".into(), serde_json::json!("command"));
    line.insert("command".into(), serde_json::json!(cmd));

    write_settings(home, &settings)?;
    Ok(status(home))
}

/// 브리지 제거 — statusLine을 보존해 둔 원래 명령으로 되돌리고, 없으면 항목을 지운다.
pub fn uninstall(home: &Path) -> Result<BridgeStatus, String> {
    let mut settings = read_settings(home);
    let inner = std::fs::read_to_string(inner_path(home))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(obj) = settings.as_object_mut() {
        match &inner {
            Some(prev) => {
                obj.insert(
                    "statusLine".into(),
                    serde_json::json!({ "type": "command", "command": prev }),
                );
            }
            None => {
                obj.remove("statusLine");
            }
        }
    }
    write_settings(home, &settings)?;
    std::fs::remove_file(inner_path(home)).ok();
    std::fs::remove_file(script_path(home)).ok();
    Ok(status(home))
}

/// settings.json 원자적 갱신 — 임시 파일에 쓰고 교체해 중간 상태로 깨지지 않게 한다.
fn write_settings(home: &Path, settings: &serde_json::Value) -> Result<(), String> {
    let path = settings_path(home);
    let tmp = path.with_extension("json.praxis-tmp");
    let body = serde_json::to_string_pretty(settings).map_err(|e| format!("직렬화 실패: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("설정 저장 실패: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("설정 교체 실패: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = crate::testtmp::dir().join(format!("praxis-bridge-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        dir
    }

    #[test]
    fn install_on_empty_settings() {
        let home = temp_home("empty");
        let st = install(&home).unwrap();
        assert!(st.installed);
        assert_eq!(st.wrapped, None);

        let settings = read_settings(&home);
        assert!(current_command(&settings)
            .unwrap()
            .contains("praxis-statusline.mjs"));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn install_preserves_existing_statusline() {
        let home = temp_home("wrap");
        std::fs::write(
            settings_path(&home),
            r#"{"statusLine":{"type":"command","command":"node ~/.claude/hud/omc-hud.mjs"},"model":"opus"}"#,
        )
        .unwrap();

        let st = install(&home).unwrap();
        assert_eq!(
            st.wrapped.as_deref(),
            Some("node ~/.claude/hud/omc-hud.mjs")
        );
        // 다른 설정 키는 건드리지 않는다.
        assert_eq!(read_settings(&home).get("model").unwrap(), "opus");

        // 재설치해도 원래 명령을 잃지 않는다.
        let again = install(&home).unwrap();
        assert_eq!(
            again.wrapped.as_deref(),
            Some("node ~/.claude/hud/omc-hud.mjs")
        );
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn uninstall_restores_previous_command() {
        let home = temp_home("restore");
        std::fs::write(
            settings_path(&home),
            r#"{"statusLine":{"type":"command","command":"my-hud"}}"#,
        )
        .unwrap();
        install(&home).unwrap();

        let st = uninstall(&home).unwrap();
        assert!(!st.installed);
        assert_eq!(
            current_command(&read_settings(&home)).as_deref(),
            Some("my-hud")
        );
        assert!(!script_path(&home).exists());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn uninstall_without_previous_drops_the_key() {
        let home = temp_home("drop");
        install(&home).unwrap();
        uninstall(&home).unwrap();
        assert!(read_settings(&home).get("statusLine").is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn foreign_statusline_is_reported() {
        let home = temp_home("foreign");
        std::fs::write(
            settings_path(&home),
            r#"{"statusLine":{"type":"command","command":"other-hud"}}"#,
        )
        .unwrap();
        let st = status(&home);
        assert!(!st.installed);
        assert_eq!(st.foreign.as_deref(), Some("other-hud"));
        std::fs::remove_dir_all(&home).ok();
    }
}
