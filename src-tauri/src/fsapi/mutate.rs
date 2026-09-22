//! 파일 브라우저의 실제 조작. 워크트리 판정은 호출부(commands.rs)가 먼저 한다.
//!
//! 여기서는 **경로 검증만** 한다 — `safe_join`으로 탈출·심볼릭을 막고, 이름 자체의
//! 유효성을 본다. 어떤 작업이 이 경로를 쓰고 있는지는 `guard`의 몫이다.

use std::path::{Path, PathBuf};

/// 파일/디렉터리 이름으로 쓸 수 있는가.
///
/// `safe_join`이 대부분 걸러내지만, 사용자에게는 "허용되지 않는 경로 요소"가 아니라
/// "이름에 / 를 쓸 수 없습니다"가 나가야 한다.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        anyhow::bail!("이름이 비어 있습니다");
    }
    if trimmed == "." || trimmed == ".." {
        anyhow::bail!("이름에 . 또는 .. 를 쓸 수 없습니다");
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
        anyhow::bail!("이름에 / \\ 를 쓸 수 없습니다");
    }
    Ok(())
}

/// `dir` 아래에 `name`으로 결합한 검증된 경로. 이미 있으면 에러(덮어쓰기 금지).
///
/// 존재 확인은 `symlink_metadata` — `exists()`는 dangling symlink에서 false라,
/// 링크가 걸린 자리에 새 파일을 만들려다 조용히 실패한다.
fn fresh_target(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    validate_name(name)?;
    let target = super::safe_join(dir, name.trim())?;
    if target.symlink_metadata().is_ok() {
        anyhow::bail!("같은 이름이 이미 있습니다");
    }
    Ok(target)
}

pub fn create_file(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let target = fresh_target(dir, name)?;
    std::fs::File::create(&target)?;
    Ok(target)
}

pub fn create_dir(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let target = fresh_target(dir, name)?;
    std::fs::create_dir(&target)?;
    Ok(target)
}

/// 같은 부모 안에서 이름만 바꾼다. 이동은 하지 않는다.
pub fn rename(path: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("상위 디렉터리가 없습니다"))?;
    let target = fresh_target(parent, name)?;
    std::fs::rename(path, &target)?;
    Ok(target)
}

/// OS 휴지통으로 보낸다. 영구 삭제 경로는 이 모듈에 두지 않는다(설계 0024 D3).
pub fn trash(path: &Path) -> anyhow::Result<()> {
    // 존재 확인은 symlink_metadata로 — dangling symlink도 휴지통에 보낼 수 있어야 한다.
    path.symlink_metadata()?;
    // `::trash`로 절대 경로 지정 — 이 함수 이름과 크레이트 이름이 같아 읽는 사람이 헷갈린다.
    ::trash::delete(path).map_err(|e| anyhow::anyhow!("휴지통으로 보내지 못했습니다: {e}"))
}

/// `src`를 `dest_dir` 아래로 복사한다.
///
/// **같은 폴더일 때만** 이름 충돌을 "사본" 접미사로 회피한다(PRD 0006 F-05 · §5.5).
/// 다른 폴더로의 붙여넣기에서 충돌하면 이름을 바꾸지 않고 에러다 — 사용자가 지정하지 않은
/// 이름의 파일이 조용히 생기면, 붙여넣은 것과 원래 있던 것 중 무엇이 무엇인지 알 수 없다.
pub fn copy_into(src: &Path, dest_dir: &Path) -> anyhow::Result<PathBuf> {
    let src_canon = src.canonicalize()?;
    let dest_canon = dest_dir.canonicalize()?;
    // DR-P2 — 자기 자신이나 자기 하위로 복사하면 무한 재귀다.
    if dest_canon.starts_with(&src_canon) {
        anyhow::bail!("자기 자신 또는 하위 폴더로는 복사할 수 없습니다");
    }
    let name = src_canon
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("이름을 알 수 없습니다"))?
        .to_string_lossy()
        .into_owned();
    let target = if src_canon.parent() == Some(dest_canon.as_path()) {
        super::safe_join(&dest_canon, &available_name(&dest_canon, &name))?
    } else {
        // 다른 폴더 — `fresh_target`이 충돌을 덮어쓰기 대신 에러로 만든다.
        fresh_target(&dest_canon, &name)?
    };
    if src_canon.is_dir() {
        copy_dir_recursive(&src_canon, &target)?;
    } else {
        std::fs::copy(&src_canon, &target)?;
    }
    Ok(target)
}

/// **같은 폴더 복제 전용** 이름. 없으면 그대로, 이미 있으면 "이름 사본", "이름 사본 2"… 로 올린다.
///
/// 확장자 앞에 붙인다 — `a.tar.gz` → `a 사본.tar.gz`. 프론트 `copyName`과 같은 규칙이며,
/// 프론트 쪽은 표시용 미리보기고 여기가 실제 결과다.
/// 다른 폴더 붙여넣기에서는 부르지 않는다 — `copy_into`의 분기를 볼 것.
fn available_name(dir: &Path, name: &str) -> String {
    if dir.join(name).symlink_metadata().is_err() {
        return name.to_string();
    }
    let (stem, ext) = split_compound_ext(name);
    for n in 1..1000 {
        let suffix = if n == 1 {
            " 사본".to_string()
        } else {
            format!(" 사본 {n}")
        };
        let candidate = format!("{stem}{suffix}{ext}");
        if dir.join(&candidate).symlink_metadata().is_err() {
            return candidate;
        }
    }
    format!("{stem} 사본{ext}")
}

/// `a.tar.gz` → `("a", ".tar.gz")`. 앞의 점(`.env`)은 확장자로 보지 않는다.
fn split_compound_ext(name: &str) -> (String, String) {
    let body = name.strip_prefix('.').map(|rest| rest.to_string());
    let (lead, work) = match &body {
        Some(rest) => (".", rest.as_str()),
        None => ("", name),
    };
    match work.find('.') {
        Some(idx) if idx > 0 => (format!("{lead}{}", &work[..idx]), work[idx..].to_string()),
        _ => (format!("{lead}{work}"), String::new()),
    }
}

/// 심볼릭 링크는 따라가지 않고 건너뛴다 — worktree 부트스트랩 복사와 같은 계약.
fn copy_dir_recursive(src: &Path, dest: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        let to = dest.join(entry.file_name());
        if kind.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 저장소는 tempfile을 쓰지 않는다 — `fsapi/mod.rs:332`·`knowledge/tests/obsidian.rs:10`
    /// 과 같은 패턴. 원자 카운터로 병렬 테스트 간 충돌을 막는다.
    fn tmp_root() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = crate::testtmp::dir().join(format!(
            "praxis-mutate-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_file_rejects_existing_name() {
        let dir = tmp_root();
        create_file(&dir, "a.txt").unwrap();
        let err = create_file(&dir, "a.txt").unwrap_err();
        assert!(err.to_string().contains("이미 있습니다"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_file_rejects_slash_in_name() {
        let dir = tmp_root();
        let err = create_file(&dir, "a/b.txt").unwrap_err();
        assert!(err.to_string().contains("이름에"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_rejects_collision_without_overwriting() {
        let dir = tmp_root();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        assert!(rename(&dir.join("a.txt"), "b.txt").is_err());
        // 덮어쓰지 않았는지 — 원본과 대상이 모두 그대로여야 한다.
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b");
        assert!(dir.join("a.txt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_rejects_dest_inside_source() {
        let dir = tmp_root();
        let src = dir.join("a");
        std::fs::create_dir(&src).unwrap();
        let inner = src.join("b");
        std::fs::create_dir(&inner).unwrap();
        // DR-P2 — 자기 하위로 복사하면 무한 재귀다.
        assert!(copy_into(&src, &inner).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_dir_skips_symlinks() {
        let dir = tmp_root();
        let src = dir.join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("real.txt"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", src.join("link")).unwrap();
        let dest = dir.join("dest");
        std::fs::create_dir(&dest).unwrap();
        let out = copy_into(&src, &dest).unwrap();
        assert!(out.join("real.txt").exists());
        assert!(
            out.join("link").symlink_metadata().is_err(),
            "심볼릭 링크는 건너뛴다"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_into_same_dir_suffixes_before_compound_ext() {
        let dir = tmp_root();
        std::fs::write(dir.join("a.tar.gz"), "x").unwrap();
        let out = copy_into(&dir.join("a.tar.gz"), &dir).unwrap();
        assert_eq!(out.file_name().unwrap(), "a 사본.tar.gz");
        // 두 번째 복사는 번호를 올린다.
        let again = copy_into(&dir.join("a.tar.gz"), &dir).unwrap();
        assert_eq!(again.file_name().unwrap(), "a 사본 2.tar.gz");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// PRD 0006 F-05 — **다른** 폴더로의 붙여넣기에서 충돌하면 이름을 바꾸지 않고 멈춘다.
    /// 같은 폴더 복제(위 테스트)와 갈리는 지점이다.
    #[test]
    fn copy_into_other_dir_errors_on_conflict() {
        let dir = tmp_root();
        let src = dir.join("src");
        let dest = dir.join("dest");
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dest).unwrap();
        std::fs::write(src.join("a.txt"), "새것").unwrap();
        std::fs::write(dest.join("a.txt"), "원래것").unwrap();

        let err = copy_into(&src.join("a.txt"), &dest).unwrap_err();
        assert!(err.to_string().contains("이미 있습니다"));
        // 덮어쓰지도, "a 사본.txt"를 만들지도 않았는지.
        assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "원래것");
        assert!(dest.join("a 사본.txt").symlink_metadata().is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn split_compound_ext_treats_dotfile_as_name() {
        assert_eq!(
            split_compound_ext(".env"),
            (".env".to_string(), String::new())
        );
        assert_eq!(
            split_compound_ext("a.tar.gz"),
            ("a".to_string(), ".tar.gz".to_string())
        );
        assert_eq!(
            split_compound_ext("README"),
            ("README".to_string(), String::new())
        );
    }
}
