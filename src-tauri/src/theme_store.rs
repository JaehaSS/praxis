//! 커스텀 테마 파일 저장소 — app_config_dir()/themes/<id>.json (설계 0049).
use std::fs;
use std::path::Path;

const MAX_BYTES: usize = 262_144; // 256KB — 팔레트 JSON은 ~1KB, 여유 100배 (save/import 공용)

pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("custom-").is_some_and(|rest| {
        !rest.is_empty()
            && id.len() <= 64
            && rest.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

pub fn list(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| match fs::read_to_string(e.path()) {
            Ok(json) => Some(json),
            // 한 파일이 깨져도 나머지 목록은 살린다 — 대신 조용히 사라지지는 않게 남긴다.
            Err(error) => {
                eprintln!("테마 파일 읽기 실패({}) — 목록에서 제외: {error}", e.path().display());
                None
            }
        })
        .collect();
    out.sort(); // 결정적 순서 — 프론트 정렬 부담 제거
    out
}

pub fn save(dir: &Path, id: &str, json: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("잘못된 테마 id: {id}"));
    }
    if json.len() > MAX_BYTES {
        return Err("테마 파일이 너무 큽니다".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{id}.json.tmp"));
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join(format!("{id}.json"))).map_err(|e| e.to_string())
}

pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("잘못된 테마 id: {id}"));
    }
    fs::remove_file(dir.join(format!("{id}.json"))).map_err(|e| e.to_string())
}

/// 저장된 테마 파일을 `dest`로 복사한다. 파일명 규약(`{id}.json`)은 여기서만 안다.
pub fn export(dir: &Path, id: &str, dest: &Path) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("잘못된 테마 id: {id}"));
    }
    fs::copy(dir.join(format!("{id}.json")), dest).map_err(|e| e.to_string())?;
    Ok(())
}

/// 저장소 밖 경로에서 테마 spec 원문을 읽는다. 저장은 별도 `save` 호출.
///
/// 크기는 metadata로 먼저 본다 — 읽고 나서 재는 것은 이미 그만큼 메모리에 올린 뒤다.
pub fn read_external(src: &Path) -> Result<String, String> {
    let len = fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > MAX_BYTES as u64 {
        return Err("테마 파일이 너무 큽니다".into());
    }
    fs::read_to_string(src).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 각 테스트가 고유 임시 디렉터리를 갖도록 pid + 나노초로 유니크화한다 (tempfile 미의존).
    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        let dir = crate::testtmp::dir().join(format!("praxis-theme-store-{tag}-{}-{nanos}", std::process::id()));
        dir
    }

    #[test]
    fn valid_id_rejects_traversal_and_bad_prefix() {
        assert!(!valid_id("custom-../x"));
        assert!(!valid_id("praxis-dark"));
        assert!(!valid_id("custom-"));
        assert!(valid_id("custom-my-dusk"));
    }

    #[test]
    fn save_list_delete_roundtrip() {
        let dir = temp_dir("roundtrip");
        let id = "custom-my-dusk";
        let json = r#"{"schema":1,"id":"custom-my-dusk"}"#;

        save(&dir, id, json).expect("save should succeed");
        let listed = list(&dir);
        assert_eq!(listed, vec![json.to_string()]);

        delete(&dir, id).expect("delete should succeed");
        assert!(list(&dir).is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_overwrites_atomically_without_leftover_tmp() {
        let dir = temp_dir("overwrite");
        let id = "custom-my-dusk";
        save(&dir, id, r#"{"schema":1,"v":1}"#).expect("first save should succeed");
        save(&dir, id, r#"{"schema":1,"v":2}"#).expect("second save should succeed");

        let listed = list(&dir);
        assert_eq!(listed, vec![r#"{"schema":1,"v":2}"#.to_string()]);

        let tmp = dir.join(format!("{id}.json.tmp"));
        assert!(!tmp.exists(), "tmp file must not survive a successful save");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_rejects_invalid_id() {
        let dir = temp_dir("invalid-id");
        let err = save(&dir, "praxis-dark", "{}").unwrap_err();
        assert!(err.contains("잘못된 테마 id"));
    }

    #[test]
    fn save_rejects_oversize_but_accepts_the_exact_limit() {
        let dir = temp_dir("size");
        let id = "custom-my-dusk";

        let err = save(&dir, id, &"a".repeat(MAX_BYTES + 1)).unwrap_err();
        assert!(err.contains("테마 파일이 너무 큽니다"));

        save(&dir, id, &"a".repeat(MAX_BYTES)).expect("exactly MAX_BYTES must be accepted");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_rejects_invalid_id_and_missing_file() {
        let dir = temp_dir("delete-err");
        let err = delete(&dir, "praxis-dark").unwrap_err();
        assert!(err.contains("잘못된 테마 id"));
        assert!(delete(&dir, "custom-nope").is_err(), "deleting a missing theme must error");
    }

    #[test]
    fn list_returns_only_json_files() {
        let dir = temp_dir("list-filter");
        let json = r#"{"schema":1,"id":"custom-a"}"#;
        save(&dir, "custom-a", json).expect("save should succeed");
        fs::write(dir.join("custom-b.json.tmp"), "half written").expect("tmp write should succeed");
        fs::write(dir.join("notes.txt"), "not a theme").expect("txt write should succeed");

        assert_eq!(list(&dir), vec![json.to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_id_boundaries() {
        let at_limit = format!("custom-{}", "a".repeat(64 - "custom-".len()));
        assert_eq!(at_limit.len(), 64);
        assert!(valid_id(&at_limit));
        assert!(!valid_id(&format!("{at_limit}a")));
        assert!(!valid_id("custom-Dusk"));
        assert!(valid_id("custom-9"));
    }

    #[test]
    fn export_copies_the_saved_file_to_the_destination() {
        let dir = temp_dir("export");
        let json = r#"{"schema":1,"id":"custom-my-dusk"}"#;
        save(&dir, "custom-my-dusk", json).expect("save should succeed");

        let dest = dir.join("exported.json");
        export(&dir, "custom-my-dusk", &dest).expect("export should succeed");
        assert_eq!(fs::read_to_string(&dest).expect("destination should exist"), json);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_rejects_invalid_id_and_missing_source() {
        let dir = temp_dir("export-err");
        fs::create_dir_all(&dir).expect("temp dir should be creatable");
        let dest = dir.join("exported.json");

        let err = export(&dir, "praxis-dark", &dest).unwrap_err();
        assert!(err.contains("잘못된 테마 id"));
        assert!(export(&dir, "custom-nope", &dest).is_err(), "missing source must error");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_external_reads_within_the_limit_and_rejects_beyond_it() {
        let dir = temp_dir("read-external");
        fs::create_dir_all(&dir).expect("temp dir should be creatable");

        let json = r#"{"schema":1,"id":"custom-my-dusk"}"#;
        let small = dir.join("small.json");
        fs::write(&small, json).expect("write should succeed");
        assert_eq!(read_external(&small).expect("read should succeed"), json);

        let big = dir.join("big.json");
        fs::write(&big, "a".repeat(MAX_BYTES + 1)).expect("write should succeed");
        let err = read_external(&big).unwrap_err();
        assert!(err.contains("테마 파일이 너무 큽니다"));

        let _ = fs::remove_dir_all(&dir);
    }
}
