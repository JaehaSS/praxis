//! 창고 채널의 세 설정 — 위키 폴더, 정리 세션이 부를 스킬, 위키 진입 문서.
//!
//! 셋 다 `settings(key,value)`에 산다. 폴더는 창고 루트 기준 **상대 경로**이고
//! 여러 단계여도 된다(`문서/기술-위키/wiki`). 하드코딩하던 시절에는 루트 바로 아래
//! `wiki/`만 위키로 보여, 실제 창고 구조가 조금만 달라도 위키 문서가 자료로
//! 등록됐다(계획 2026-09-13).

use sqlx::SqlitePool;

pub const SETTING_WIKI_DIR: &str = "vault_wiki_dir";
pub const SETTING_ORGANIZER_SKILL: &str = "vault_organizer_skill";
pub const SETTING_WIKI_HOME: &str = "vault_wiki_home";
pub const DEFAULT_WIKI_DIR: &str = "wiki";
pub const DEFAULT_ORGANIZER_SKILL: &str = "wiki-organizer";
pub const DEFAULT_WIKI_HOME: &str = "위키-시작.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultSettings {
    /// 창고 루트 기준 상대 경로. 선행·후행 `/`가 없고 빈 문자열이 아니다.
    pub wiki_dir: String,
    /// `.claude/skills/<이름>/SKILL.md`의 `<이름>`. 경로 구분자를 담지 않는다.
    pub organizer_skill: String,
    /// 위키를 열었을 때 먼저 보여줄 문서. 창고 루트 기준 상대 경로거나
    /// 파일 이름 하나다(`위키-시작.md`) — 이름만 주면 위키 폴더가 어디로
    /// 옮겨가도 따라간다. 찾지 못하면 화면이 스스로 진입 문서를 고른다.
    pub wiki_home: String,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            wiki_dir: DEFAULT_WIKI_DIR.to_owned(),
            organizer_skill: DEFAULT_ORGANIZER_SKILL.to_owned(),
            wiki_home: DEFAULT_WIKI_HOME.to_owned(),
        }
    }
}

/// 상대 경로로 정규화한다 — 선행·후행·중복 `/`와 `.`를 떼고, 빈 값은 기본값이다.
/// `..`는 **거부한다**: 기본값으로 조용히 물러서면 창고 밖을 가리킨 설정이
/// 아무 말 없이 `wiki/`로 바뀌어, 화면과 실제가 달라진다.
pub fn normalize_wiki_dir(raw: &str) -> Result<String, String> {
    let raw = raw.trim().replace('\\', "/");
    if raw.is_empty() {
        return Ok(DEFAULT_WIKI_DIR.to_owned());
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part.trim() {
            "" | "." => continue,
            ".." => return Err("위키 폴더에 `..`를 쓸 수 없습니다".into()),
            part => parts.push(part.to_owned()),
        }
    }
    if parts.is_empty() {
        return Ok(DEFAULT_WIKI_DIR.to_owned());
    }
    Ok(parts.join("/"))
}

/// 진입 문서는 창고 안의 Markdown 하나다 — `wiki_dir`과 같은 규칙으로 정규화하고,
/// `..`와 `.md`가 아닌 값은 거부한다. 빈 값은 기본값(`위키-시작.md`)이다.
pub fn normalize_wiki_home(raw: &str) -> Result<String, String> {
    let raw = raw.trim().replace('\\', "/");
    if raw.is_empty() {
        return Ok(DEFAULT_WIKI_HOME.to_owned());
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        match part.trim() {
            "" | "." => continue,
            ".." => return Err("진입 문서에 `..`를 쓸 수 없습니다".into()),
            part => parts.push(part.to_owned()),
        }
    }
    if parts.is_empty() {
        return Ok(DEFAULT_WIKI_HOME.to_owned());
    }
    let path = parts.join("/");
    if !path.to_lowercase().ends_with(".md") {
        return Err("진입 문서는 `.md` 파일이어야 합니다".into());
    }
    Ok(path)
}

/// 스킬 **이름**이지 경로가 아니다 — 경로 구분자를 받으면
/// `.claude/skills/<이름>/SKILL.md`가 스킬 디렉터리 밖을 가리킬 수 있다.
pub fn normalize_organizer_skill(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(DEFAULT_ORGANIZER_SKILL.to_owned());
    }
    if raw.contains('/') || raw.contains('\\') || raw == ".." || raw == "." {
        return Err("정리 스킬은 경로가 아니라 이름이어야 합니다".into());
    }
    Ok(raw.to_owned())
}

/// 저장된 값을 읽는다. 읽기는 실패해선 안 되는 경로(스캔·세션 열기)에서 불리므로
/// 값이 없거나 깨졌으면 기본값으로 간다 — 쓰기(`store`)가 이미 막고 있다.
pub async fn load(pool: &SqlitePool) -> VaultSettings {
    let wiki_dir = crate::db::get_setting(pool, SETTING_WIKI_DIR)
        .await
        .ok()
        .flatten()
        .and_then(|value| normalize_wiki_dir(&value).ok())
        .unwrap_or_else(|| DEFAULT_WIKI_DIR.to_owned());
    let organizer_skill = crate::db::get_setting(pool, SETTING_ORGANIZER_SKILL)
        .await
        .ok()
        .flatten()
        .and_then(|value| normalize_organizer_skill(&value).ok())
        .unwrap_or_else(|| DEFAULT_ORGANIZER_SKILL.to_owned());
    let wiki_home = crate::db::get_setting(pool, SETTING_WIKI_HOME)
        .await
        .ok()
        .flatten()
        .and_then(|value| normalize_wiki_home(&value).ok())
        .unwrap_or_else(|| DEFAULT_WIKI_HOME.to_owned());
    VaultSettings {
        wiki_dir,
        organizer_skill,
        wiki_home,
    }
}

/// 정규화한 뒤 저장하고, 저장된 그대로를 돌려준다.
pub async fn store(
    pool: &SqlitePool,
    wiki_dir: &str,
    organizer_skill: &str,
    wiki_home: &str,
) -> Result<VaultSettings, String> {
    let settings = VaultSettings {
        wiki_dir: normalize_wiki_dir(wiki_dir)?,
        organizer_skill: normalize_organizer_skill(organizer_skill)?,
        wiki_home: normalize_wiki_home(wiki_home)?,
    };
    crate::db::set_setting(pool, SETTING_WIKI_DIR, &settings.wiki_dir)
        .await
        .map_err(|error| error.to_string())?;
    crate::db::set_setting(pool, SETTING_ORGANIZER_SKILL, &settings.organizer_skill)
        .await
        .map_err(|error| error.to_string())?;
    crate::db::set_setting(pool, SETTING_WIKI_HOME, &settings.wiki_home)
        .await
        .map_err(|error| error.to_string())?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_dir_normalizes_slashes_and_refuses_to_climb_out() {
        assert_eq!(normalize_wiki_dir("wiki").unwrap(), "wiki");
        assert_eq!(normalize_wiki_dir("  /wiki/  ").unwrap(), "wiki");
        assert_eq!(
            normalize_wiki_dir("/문서/기술-위키/wiki/").unwrap(),
            "문서/기술-위키/wiki"
        );
        assert_eq!(normalize_wiki_dir("개인//wiki").unwrap(), "개인/wiki");
        assert_eq!(normalize_wiki_dir("./wiki").unwrap(), "wiki");
        // 빈 값은 기본값이다 — 화면이 비워 보내도 위키가 사라지지 않는다.
        assert_eq!(normalize_wiki_dir("").unwrap(), DEFAULT_WIKI_DIR);
        assert_eq!(normalize_wiki_dir("   ").unwrap(), DEFAULT_WIKI_DIR);
        assert_eq!(normalize_wiki_dir("/").unwrap(), DEFAULT_WIKI_DIR);
        // `..`만은 기본값으로 물러서지 않는다.
        assert!(normalize_wiki_dir("../wiki").is_err());
        assert!(normalize_wiki_dir("개인/../../wiki").is_err());
    }

    #[test]
    fn wiki_home_keeps_a_bare_file_name_and_refuses_non_markdown() {
        // 이름만 주는 쪽이 기본 사용법이다 — 위키 폴더를 옮겨도 따라간다.
        assert_eq!(normalize_wiki_home("위키-시작.md").unwrap(), "위키-시작.md");
        assert_eq!(
            normalize_wiki_home(" /문서/기술-위키/wiki/위키-시작.md ").unwrap(),
            "문서/기술-위키/wiki/위키-시작.md"
        );
        assert_eq!(normalize_wiki_home("./시작.MD").unwrap(), "시작.MD");
        assert_eq!(normalize_wiki_home("").unwrap(), DEFAULT_WIKI_HOME);
        assert_eq!(normalize_wiki_home("   ").unwrap(), DEFAULT_WIKI_HOME);
        assert_eq!(normalize_wiki_home("/").unwrap(), DEFAULT_WIKI_HOME);
        // 폴더나 창고 밖은 진입 문서가 될 수 없다.
        assert!(normalize_wiki_home("wiki").is_err());
        assert!(normalize_wiki_home("../위키-시작.md").is_err());
    }

    #[test]
    fn organizer_skill_is_a_name_not_a_path() {
        assert_eq!(
            normalize_organizer_skill("  knowledge-harness  ").unwrap(),
            "knowledge-harness"
        );
        assert_eq!(normalize_organizer_skill("").unwrap(), DEFAULT_ORGANIZER_SKILL);
        assert!(normalize_organizer_skill("../evil").is_err());
        assert!(normalize_organizer_skill("a/b").is_err());
    }

    #[tokio::test]
    async fn store_and_load_round_trip_through_the_settings_table() {
        let pool = crate::knowledge::tests::raw_pool().await;
        // 아무것도 저장하지 않았으면 기본값이다.
        assert_eq!(load(&pool).await, VaultSettings::default());
        let stored = store(
            &pool,
            " /문서/기술-위키/wiki/ ",
            " knowledge-harness ",
            " 문서/기술-위키/wiki/시작.md ",
        )
        .await
        .unwrap();
        assert_eq!(stored.wiki_dir, "문서/기술-위키/wiki");
        assert_eq!(stored.organizer_skill, "knowledge-harness");
        assert_eq!(stored.wiki_home, "문서/기술-위키/wiki/시작.md");
        assert_eq!(load(&pool).await, stored);
        // 빈 값은 기본값으로 되돌린다 — 화면의 "비우기"가 초기화다.
        let cleared = store(&pool, "", "", "").await.unwrap();
        assert_eq!(cleared, VaultSettings::default());
        assert_eq!(load(&pool).await, VaultSettings::default());
        // 거부된 값은 저장되지 않는다 — 셋 중 하나만 어긋나도 전부 남지 않는다.
        assert!(store(&pool, "../밖", "wiki-organizer", "위키-시작.md")
            .await
            .is_err());
        assert!(store(&pool, "wiki", "wiki-organizer", "위키-시작")
            .await
            .is_err());
        assert_eq!(load(&pool).await, VaultSettings::default());
    }
}
