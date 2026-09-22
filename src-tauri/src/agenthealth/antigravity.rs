//! Antigravity Hub의 "재시작하면 적용될 업데이트" 감지.
//!
//! **자동으로 설치하지 않는다.** Hub의 설치는 앱 종료를 요구하는데(Squirrel/ShipIt), 남의 앱을
//! 강제로 종료하면 사용자가 그 앱에서 하던 일이 사라진다. 되돌릴 수 없는 손실을 자동화에 넣지
//! 않는다 — `agenthealth`가 Antigravity를 벤더 목록에서 아예 제외한 것과 같은 판단이다.
//! 우리가 할 수 있고 해도 되는 일은 **알리는 것**뿐이다.
//!
//! 알림에 값이 있다는 근거: 이 개발기에서 Hub가 8/24부터 종료되지 않아 2.11.0 설치가 이틀간
//! 밀렸다. 그동안 ShipIt은 매시간 install request만 반복했고 업데이터는 166MB를 계속 다시
//! 받았다. 아무도 몰랐던 이유는 **어디에도 표시되지 않았기 때문**이다.
//!
//! ## 판정 근거를 고른 이유
//!
//! `pending/` 디렉터리의 존재로 판정하지 않는다. 설치를 끝낸 뒤에도 남기 때문이다 — 실측으로
//! 2.11.0 설치를 완료한 직후에도 8/27자 `pending/Antigravity.zip`이 그대로 있었다. 그것을
//! 신호로 쓰면 영구히 "업데이트 있음"이 뜬다. `update-info.json`도 못 쓴다. 파일명·sha512·
//! 관리자 권한 필요 여부뿐이고 **버전이 없다.**
//!
//! 대신 로그의 `Update downloaded: <버전>`을 설치 버전과 견준다. "다운로드가 끝났다"가 곧
//! "재시작하면 적용된다"라서 우리가 답해야 할 질문과 의미가 정확히 겹친다.

use serde::Serialize;

use super::detect::{is_newer, parse_version};

/// Hub 앱 번들의 Info.plist. 다른 위치에 설치했다면 읽지 못하고 `installed`가 None이 된다 —
/// 그때는 모른다고 답한다(추측해서 없는 업데이트를 권하지 않는다).
#[cfg(target_os = "macos")]
const APP_PLIST: &str = "/Applications/Antigravity.app/Contents/Info.plist";

/// electron-log가 쓰는 Hub 로그. HOME 기준 상대 경로.
#[cfg(target_os = "macos")]
const LOG_REL: &str = "Library/Logs/Antigravity/main.log";

/// 로그 끝에서 읽을 최대 바이트. 업데이트 확인은 한 시간에 한 번이라 이 정도면 며칠치가 들어온다.
#[cfg(target_os = "macos")]
const LOG_TAIL_BYTES: u64 = 64 * 1024;

/// 업데이터가 남긴 다운로드 완료 표식.
const DOWNLOADED_MARK: &str = "[AutoUpdater] Update downloaded:";

/// Hub의 업데이트 상태.
///
/// 벤더 CLI(`VendorHealth`)와 타입을 나누는 이유는 **할 수 있는 일이 다르기 때문**이다.
/// 저쪽은 버튼이 명령을 실행하지만 이쪽은 사용자가 앱을 재시작하는 수밖에 없다. 같은 모양으로
/// 두면 UI가 없는 버튼을 그리게 된다.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct HubUpdate {
    /// 설치된 버전. 앱이 없거나 plist를 읽지 못하면 None.
    pub installed: Option<String>,
    /// 업데이터가 이미 받아둔 버전.
    pub downloaded: Option<String>,
    /// 받아둔 것이 설치된 것보다 새로운가 — 참이면 Hub 재시작만으로 적용된다.
    pub restart_required: bool,
}

/// XML plist에서 문자열 값 하나를 꺼낸다.
///
/// `defaults read`를 부르지 않는 이유는 프로세스를 띄우지 않고도 되기 때문이고, 무엇보다
/// **순수 함수라야 테스트할 수 있기 때문**이다. 바이너리 plist는 파싱하지 못해 None으로
/// 떨어지는데, 그 경우 버전을 모른다고 답하는 것이 틀린 값을 만드는 것보다 낫다.
///
/// 키 **바로 다음** 태그가 `<string>`일 때만 값으로 인정한다. 문서 끝까지 아무 `<string>`이나
/// 집으면 `<key>버전</key><false/>` 뒤에 오는 무관한 키의 값을 버전이라고 답하게 된다.
pub fn plist_string(xml: &str, key: &str) -> Option<String> {
    let key_tag = format!("<key>{key}</key>");
    let after = xml[xml.find(&key_tag)? + key_tag.len()..].trim_start();
    let value = after.strip_prefix("<string>")?;
    let end = value.find("</string>")?;
    let value = value[..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 로그에서 **마지막으로** 다운로드가 끝난 버전.
///
/// 마지막을 취하는 이유: 업데이터는 한 시간마다 같은 줄을 다시 쓴다. 가장 최근 것만이 지금
/// 대기 중인 버전을 뜻한다.
///
/// 버전으로 읽히지 않는 줄은 건너뛰고 **계속 뒤로 간다.** 로그 마지막 줄은 기록 도중 잘려
/// 있을 수 있는데(tail 로 읽으므로 흔하다), 거기서 멈추면 바로 앞의 멀쩡한 버전까지 함께
/// 버려진다.
pub fn parse_downloaded_version(log: &str) -> Option<String> {
    log.lines().rev().find_map(|line| {
        let (_, rest) = line.split_once(DOWNLOADED_MARK)?;
        parse_version(rest)
    })
}

/// 설치 버전과 대기 중인 버전을 견준다. 한쪽이라도 모르면 재시작을 권하지 않는다.
fn decide(installed: Option<String>, downloaded: Option<String>) -> HubUpdate {
    let restart_required = match (installed.as_deref(), downloaded.as_deref()) {
        (Some(installed), Some(downloaded)) => is_newer(installed, downloaded),
        _ => false,
    };
    HubUpdate {
        installed,
        downloaded,
        restart_required,
    }
}

/// 현재 상태를 읽는다. macOS 밖에서는 항상 빈 결과 — Hub가 그 형태로 존재하지 않는다.
#[cfg(target_os = "macos")]
pub fn detect() -> HubUpdate {
    let installed = std::fs::read_to_string(APP_PLIST)
        .ok()
        .and_then(|xml| plist_string(&xml, "CFBundleShortVersionString"));
    let downloaded = crate::usage::home_dir()
        .and_then(|home| crate::usage::tail_string(&home.join(LOG_REL), LOG_TAIL_BYTES))
        .as_deref()
        .and_then(parse_downloaded_version);
    decide(installed, downloaded)
}

#[cfg(not(target_os = "macos"))]
pub fn detect() -> HubUpdate {
    HubUpdate::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 개발기의 실제 로그 형태.
    const LOG: &str = "\
[2026-08-28 08:30:10.059] [info]  [AutoUpdater] Update available: 2.11.0
[2026-08-28 08:30:10.214] [info]  [AutoUpdater] Update downloaded: 2.10.0
[2026-08-28 09:30:06.155] [info]  [AutoUpdater] Update available: 2.11.0
[2026-08-28 09:30:06.301] [info]  [AutoUpdater] Update downloaded: 2.11.0
[2026-08-28 09:30:07.713] [debug] nativeUpdater.update-downloaded";

    // ── 로그 판독 ────────────────────────────────────────────────

    #[test]
    fn takes_the_last_downloaded_version_not_the_first() {
        // 업데이터는 한 시간마다 같은 줄을 다시 쓴다. 첫 줄을 고르면 옛 버전을 권하게 된다.
        assert_eq!(parse_downloaded_version(LOG).as_deref(), Some("2.11.0"));
    }

    #[test]
    fn available_is_not_downloaded() {
        // "Update available"만 있고 다운로드가 끝나지 않았으면 재시작해도 소용없다.
        let log = "[info]  [AutoUpdater] Update available: 2.11.0";
        assert_eq!(parse_downloaded_version(log), None);
    }

    #[test]
    fn a_truncated_last_line_does_not_discard_the_good_one_before_it() {
        // tail 로 읽는 로그의 마지막 줄은 기록 도중 잘려 있을 수 있다.
        let log = "\
[info]  [AutoUpdater] Update downloaded: 2.11.0
[info]  [AutoUpdater] Update downloaded:";
        assert_eq!(parse_downloaded_version(log).as_deref(), Some("2.11.0"));
    }

    #[test]
    fn trailing_words_after_the_version_do_not_leak_into_the_notice() {
        // 형식이 바뀌어 경로 따위가 붙어도 버전만 집어야 한다 — 그대로 UI에 노출되면 안 된다.
        let log = "[info]  [AutoUpdater] Update downloaded: 2.11.0 to /tmp/x.zip";
        assert_eq!(parse_downloaded_version(log).as_deref(), Some("2.11.0"));
    }

    #[test]
    fn no_log_lines_means_unknown() {
        assert_eq!(parse_downloaded_version(""), None);
    }

    // ── plist 판독 ───────────────────────────────────────────────

    #[test]
    fn reads_the_version_out_of_an_xml_plist() {
        // Antigravity의 Info.plist는 XML이다(바이너리가 아니다).
        let xml = "\
<plist version=\"1.0\">
<dict>
	<key>CFBundleName</key>
	<string>Antigravity</string>
	<key>CFBundleShortVersionString</key>
	<string>2.11.0</string>
</dict>
</plist>";
        assert_eq!(
            plist_string(xml, "CFBundleShortVersionString").as_deref(),
            Some("2.11.0")
        );
        assert_eq!(
            plist_string(xml, "CFBundleName").as_deref(),
            Some("Antigravity")
        );
    }

    #[test]
    fn a_key_whose_value_is_not_a_string_yields_nothing() {
        // 문서 끝까지 아무 <string>이나 집으면 뒤에 오는 무관한 키의 값을 버전이라 답한다.
        let xml = "\
<dict>
	<key>CFBundleShortVersionString</key>
	<false/>
	<key>CFBundleName</key>
	<string>Antigravity</string>
</dict>";
        assert_eq!(plist_string(xml, "CFBundleShortVersionString"), None);
    }

    #[test]
    fn an_empty_or_blank_value_is_not_a_version() {
        let empty = "<dict><key>V</key><string></string></dict>";
        let blank = "<dict><key>V</key><string>   </string></dict>";
        assert_eq!(plist_string(empty, "V"), None);
        assert_eq!(plist_string(blank, "V"), None);
    }

    #[test]
    fn a_missing_key_is_not_an_empty_string() {
        let xml = "<dict><key>CFBundleName</key><string>Antigravity</string></dict>";
        assert_eq!(plist_string(xml, "CFBundleShortVersionString"), None);
    }

    #[test]
    fn binary_plist_yields_nothing_rather_than_garbage() {
        // 바이너리 plist를 문자열로 읽으면 태그가 없다 — 모른다고 답해야 한다.
        assert_eq!(
            plist_string("bplist00\u{0}\u{1}rubbish", "CFBundleShortVersionString"),
            None
        );
    }

    // ── 판정 ─────────────────────────────────────────────────────

    #[test]
    fn restart_is_required_only_when_the_download_is_newer() {
        // 2.9.1 → 2.11.0. 문자열 비교였다면 "2.11.0" < "2.9.1"이라 놓쳤을 자리다.
        assert!(decide(Some("2.9.1".into()), Some("2.11.0".into())).restart_required);
    }

    #[test]
    fn installing_the_pending_update_clears_the_flag() {
        // 설치를 끝내도 pending/ 디렉터리는 남는다. 그것을 신호로 썼다면 여기서 계속 참이었다.
        assert!(!decide(Some("2.11.0".into()), Some("2.11.0".into())).restart_required);
    }

    #[test]
    fn an_older_download_never_asks_for_a_restart() {
        // 로그에는 옛 다운로드 줄이 남아 있다(픽스처의 2.10.0). 최신을 이미 깔았다면 조용해야 한다.
        assert!(!decide(Some("2.11.0".into()), Some("2.10.0".into())).restart_required);
    }

    #[test]
    fn an_unknown_side_never_prompts_a_restart() {
        // 앱을 못 찾았거나 로그가 없을 때 재시작을 권하면 근거 없는 요구가 된다.
        assert!(!decide(None, Some("2.11.0".into())).restart_required);
        assert!(!decide(Some("2.11.0".into()), None).restart_required);
        assert!(!decide(None, None).restart_required);
    }
}
