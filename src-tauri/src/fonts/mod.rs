//! 시스템 폰트 열거 — 설정 패널의 코드/UI 폰트 콤보에 사용.
//! (설계 정본: docs/designs/0007.2026-07-09-font-settings-design.md §1)

use serde::Serialize;

/// 시스템 폰트 1개 패밀리. monospace는 코드 폰트 우선 정렬용 힌트(필터 아님 — 설계 0007 리스크 참조).
#[derive(Debug, Clone, Serialize)]
pub struct FontInfo {
    pub family: String,
    pub monospace: bool,
}

/// (family, monospaced) face 목록 → 패밀리 dedupe(한 face라도 mono면 mono), 모노 우선 +
/// 이름순 정렬, 빈 이름 제외. 순수 함수(단위테스트 대상).
pub fn organize(faces: Vec<(String, bool)>) -> Vec<FontInfo> {
    let mut merged: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for (family, monospaced) in faces {
        if family.trim().is_empty() {
            continue;
        }
        merged
            .entry(family)
            .and_modify(|mono| *mono |= monospaced)
            .or_insert(monospaced);
    }

    let mut out: Vec<FontInfo> = merged
        .into_iter()
        .map(|(family, monospace)| FontInfo { family, monospace })
        .collect();
    out.sort_by(|a, b| {
        b.monospace
            .cmp(&a.monospace)
            .then_with(|| a.family.cmp(&b.family))
    });
    out
}

/// fontdb 스캔 → organize. (/Library/Fonts, ~/Library/Fonts 포함)
pub fn list_system_fonts() -> Vec<FontInfo> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    organize(
        db.faces()
            .map(|f| {
                (
                    f.families
                        .first()
                        .map(|(name, _)| name.clone())
                        .unwrap_or_default(),
                    f.monospaced,
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organize_dedupes_family_merging_monospace_with_or() {
        let faces = vec![
            ("Menlo".to_string(), true),
            ("Menlo".to_string(), false), // 한 face라도 mono면 패밀리 전체 mono.
        ];
        let out = organize(faces);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].family, "Menlo");
        assert!(out[0].monospace);
    }

    #[test]
    fn organize_sorts_monospace_first_then_by_name() {
        let faces = vec![
            ("Zapfino".to_string(), false),
            ("Arial".to_string(), false),
            ("Menlo".to_string(), true),
            ("Courier New".to_string(), true),
        ];
        let out = organize(faces);
        let names: Vec<&str> = out.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(names, vec!["Courier New", "Menlo", "Arial", "Zapfino"]);
    }

    #[test]
    fn organize_excludes_empty_family_names() {
        let faces = vec![
            ("".to_string(), false),
            ("   ".to_string(), true),
            ("Inter".to_string(), false),
        ];
        let out = organize(faces);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].family, "Inter");
    }

    #[test]
    fn list_system_fonts_is_not_empty() {
        // 시스템 의존 스모크: 테스트 실행 머신에 폰트가 하나도 없는 경우는 실질적으로 없음.
        assert!(!list_system_fonts().is_empty());
    }
}
