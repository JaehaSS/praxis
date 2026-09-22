//! 스킬 시스템 통합 테스트 — 공개 API 계약. `cargo test`
//!
//! 스킬의 정본은 각 에이전트 디렉터리다. 여기서 검증하는 것은 두 가지다:
//! 목록이 **벤더 디렉터리를 실측**하는가, 발동이 **대상이 못 할 때만 확장**하는가.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::convo::Vendor;
use praxis_lib::skills;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn tmp_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = temp_root::dir().join(format!(
        "praxis-skills-test-{}-{}-{}",
        std::process::id(),
        n,
        label
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_at(path: PathBuf, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn claude_skill(base: &Path, name: &str, content: &str) {
    write_at(
        base.join(".claude")
            .join("skills")
            .join(name)
            .join("SKILL.md"),
        content,
    );
}

fn claude_command(base: &Path, name: &str, content: &str) {
    write_at(
        base.join(".claude")
            .join("commands")
            .join(format!("{name}.md")),
        content,
    );
}

fn codex_prompt(base: &Path, name: &str, content: &str) {
    write_at(
        base.join(".codex")
            .join("prompts")
            .join(format!("{name}.md")),
        content,
    );
}

fn gemini_command(base: &Path, ns: &str, name: &str, content: &str) {
    write_at(
        base.join(".gemini")
            .join("commands")
            .join(ns)
            .join(format!("{name}.toml")),
        content,
    );
}

fn praxis_copy(base: &Path, name: &str, content: &str) {
    write_at(
        base.join(".praxis")
            .join("skills")
            .join(format!("{name}.md")),
        content,
    );
}

// ── 인벤토리 ──

#[test]
fn list_spans_every_vendor_directory() {
    let home = tmp_dir("inventory");
    claude_skill(&home, "from-skills", "---\ndescription: A\n---\n본문 A");
    claude_command(&home, "from-commands", "---\ndescription: B\n---\n본문 B");
    codex_prompt(&home, "from-codex", "---\ndescription: C\n---\n본문 C");
    gemini_command(
        &home,
        "ns",
        "from-gemini",
        "description = \"D\"\nprompt = '''\n본문 D\n'''",
    );

    let list = skills::collect_skills("", &home);
    let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
    for expected in ["from-skills", "from-commands", "from-codex", "ns-from-gemini"] {
        assert!(names.contains(&expected), "{expected} 누락: {names:?}");
    }

    let by_vendor = |n: &str| {
        list.iter()
            .find(|m| m.name == n)
            .unwrap()
            .source
            .clone()
    };
    assert_eq!(by_vendor("from-skills"), "claude");
    assert_eq!(by_vendor("from-codex"), "codex");
    assert_eq!(by_vendor("ns-from-gemini"), "antigravity");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn praxis_copies_appear_only_as_orphans() {
    let home = tmp_dir("orphan");
    claude_skill(&home, "owned", "---\ndescription: 벤더\n---\n벤더 본문");
    praxis_copy(&home, "owned", "---\ndescription: 사본\n---\n사본 본문");
    praxis_copy(&home, "stranded", "---\ndescription: 남은 것\n---\n사본 본문");

    let list = skills::collect_skills("", &home);

    let owned = list.iter().find(|m| m.name == "owned").unwrap();
    assert_eq!(owned.description, "벤더", "벤더 원본이 사본을 가린다");
    assert!(!owned.orphan);
    assert_eq!(owned.homes.len(), 1);

    let stranded = list.iter().find(|m| m.name == "stranded").unwrap();
    assert!(stranded.orphan, "벤더에 없는 사본만 고아");
    assert!(stranded.homes.is_empty(), "고아는 거처가 없다");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn same_name_across_vendors_folds_into_one_row() {
    let home = tmp_dir("fold");
    claude_skill(&home, "shared", "---\ndescription: C\n---\nclaude 본문");
    codex_prompt(&home, "shared", "---\ndescription: X\n---\ncodex 본문");

    let list = skills::collect_skills("", &home);
    let rows: Vec<_> = list.iter().filter(|m| m.name == "shared").collect();
    assert_eq!(rows.len(), 1, "한 행으로 접힌다");
    assert_eq!(rows[0].homes.len(), 2, "거처는 둘 다 실린다");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn project_layer_wins_over_global() {
    let home = tmp_dir("layer-home");
    let repo = tmp_dir("layer-repo");
    claude_skill(&home, "layered", "---\ndescription: 글로벌\n---\n글로벌 본문");
    claude_skill(&repo, "layered", "---\ndescription: 프로젝트\n---\n프로젝트 본문");

    let list = skills::collect_skills(&repo.to_string_lossy(), &home);
    let m = list.iter().find(|m| m.name == "layered").unwrap();
    assert_eq!(m.description, "프로젝트");
    assert!(!m.global, "프로젝트 거처가 있으면 global이 아니다");
    assert_eq!(m.homes.len(), 2);

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&repo);
}

// ── 발동 ──

#[test]
fn claude_native_skill_is_passed_through_untouched() {
    let home = tmp_dir("native");
    claude_skill(&home, "native", "---\ndescription: d\n---\n본문");

    assert!(
        skills::resolve_message_with_home("", Vendor::Claude, "/native", &home).is_none(),
        "claude가 스스로 해석하므로 Praxis는 손대지 않는다"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn codex_gets_the_claude_body_verbatim() {
    let home = tmp_dir("bridge");
    let body = "1단계\n2단계\n3단계";
    claude_skill(&home, "steps", &format!("---\ndescription: d\n---\n{body}"));

    let out = skills::resolve_message_with_home("", Vendor::Codex, "/steps", &home).unwrap();
    assert_eq!(out, body, "확장 결과는 원본 body와 바이트 일치");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn each_vendor_prefers_its_own_directory() {
    let home = tmp_dir("prefer");
    claude_skill(&home, "dual", "---\ndescription: d\n---\nclaude 본문");
    codex_prompt(&home, "dual", "---\ndescription: d\n---\ncodex 본문");

    let out = skills::resolve_message_with_home("", Vendor::Codex, "/dual", &home).unwrap();
    assert_eq!(out, "codex 본문");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn arguments_reach_the_expanded_body() {
    let home = tmp_dir("args");
    claude_skill(&home, "target", "---\ndescription: d\n---\n대상: $ARGUMENTS");

    let out =
        skills::resolve_message_with_home("", Vendor::Codex, "/target src/a.ts", &home).unwrap();
    assert_eq!(out, "대상: src/a.ts");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn unknown_names_and_plain_text_pass_through() {
    let home = tmp_dir("plain");
    for msg in ["/nosuch", "일반 텍스트", "/BAD", "/"] {
        assert!(
            skills::resolve_message_with_home("", Vendor::Codex, msg, &home).is_none(),
            "{msg}는 원문 그대로 가야 한다"
        );
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn native_lookup_covers_skills_commands_and_project_layer() {
    let home = tmp_dir("stat-home");
    let repo = tmp_dir("stat-repo");
    claude_skill(&home, "gskill", "---\ndescription: d\n---\n본문");
    claude_command(&home, "gcmd", "---\ndescription: d\n---\n본문");
    claude_command(&repo, "pcmd", "---\ndescription: d\n---\n본문");

    assert!(skills::claude_resolves_natively("", "gskill", &home));
    assert!(skills::claude_resolves_natively("", "gcmd", &home));
    assert!(skills::claude_resolves_natively(
        &repo.to_string_lossy(),
        "pcmd",
        &home
    ));
    assert!(!skills::claude_resolves_natively("", "pcmd", &home));

    let _ = fs::remove_dir_all(&home);
    let _ = fs::remove_dir_all(&repo);
}

// ── 캐시 ──

#[test]
fn cached_listing_matches_direct_scan() {
    let home = tmp_dir("cache");
    claude_skill(&home, "cached", "---\ndescription: d\n---\n본문");

    // 캐시 히트가 스캔을 건너뛰는지는 단위 테스트가 직렬화된 채로 본다
    // (FILE_READS는 전역이라 통합 테스트의 병렬 실행에서는 델타가 오염된다).
    let direct = skills::collect_skills("", &home);
    let cached = skills::list_skills_cached("", &home);
    assert_eq!(direct.len(), cached.len());
    assert_eq!(direct[0].name, cached[0].name);

    let _ = fs::remove_dir_all(&home);
}
