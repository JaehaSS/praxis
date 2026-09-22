//! 벤더 중립 스킬 관측 — 스킬의 정본은 각 에이전트 디렉터리다.
//! Praxis는 저장하지 않고, 읽어서 보여 주며, 대상이 스스로 해석하지 못할 때만 읽어서 확장한다.
//! Tauri 비의존, `cargo test` 가능.

use crate::convo::Vendor;
pub mod experience;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

// DoS 가드. 실측 최대 스킬이 ~68KB(hallmark)라 두 배로 잡는다 — 이 값이 곧 목록
// 게이트여서, 좁게 잡으면 존재하는 스킬이 슬래시 목록에서 통째로 사라진다.
const MAX_SKILL_BYTES: u64 = 128 * 1024;

/// 파일을 연 횟수 — 캐시가 실제로 스캔을 건너뛰는지 테스트가 확인한다.
static FILE_READS: AtomicU64 = AtomicU64::new(0);

fn read_counted(path: &Path) -> Option<String> {
    FILE_READS.fetch_add(1, Ordering::Relaxed);
    std::fs::read_to_string(path).ok()
}

/// 지금까지 연 파일 수 — 테스트 전용 관측점.
pub fn file_read_count() -> u64 {
    FILE_READS.load(Ordering::Relaxed)
}

/// 스킬이 실제로 사는 곳. 같은 이름이 여러 벤더에 있으면 거처가 여럿이다.
#[derive(serde::Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillHome {
    pub vendor: String, // "claude" | "codex" | "antigravity"
    pub path: String,   // 절대 경로 — 화면이 그대로 보여 준다
    pub project: bool,  // 프로젝트 레이어면 true
}

/// 스킬 메타데이터.
/// 기존 4필드(`name`/`description`/`global`/`source`)는 프론트 계약 — 이름을 바꾸지 않는다.
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    /// `homes`가 전부 글로벌이면 true.
    pub global: bool,
    /// 주 거처의 vendor — 기존 출처 필터가 그대로 동작한다.
    pub source: String,

    /// 거처 전부. 비어 있으면 고아(Praxis 사본만 남은 것).
    pub homes: Vec<SkillHome>,
    /// 본문 크기 — 확장 시 프롬프트에 들어가는 양.
    pub bytes: u64,
    /// 원본 frontmatter의 `argument-hint`. 없으면 빈 문자열.
    pub argument_hint: String,
    /// `~/.praxis/skills/`에만 존재 — 마이그레이션이 필요하다.
    pub orphan: bool,
}

// ── 이름·파서 ─────────────────────────────────────────────────────────────────

// 유효 스킬 이름 문자: 소문자 a-z, 숫자, -, _
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// 슬래시로 부를 수 있는 이름인가. 플러그인 스킬은 `<plugin>:<name>` 두 마디다 —
/// claude CLI가 쓰는 표기 그대로여야 발동이 원문으로 넘어간다.
fn is_valid_skill_ref(name: &str) -> bool {
    match name.split_once(':') {
        Some((plugin, rest)) => is_valid_name(plugin) && is_valid_name(rest),
        None => is_valid_name(name),
    }
}

// frontmatter 펜스를 갈라 (frontmatter, body) 반환. `---` 펜스가 없으면 None.
fn fm_block(content: &str) -> Option<(&str, &str)> {
    let after = content.strip_prefix("---")?;
    let rest = after
        .strip_prefix('\n')
        .or_else(|| after.strip_prefix("\r\n"))?;
    let end_pos = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    let fm = &rest[..end_pos];
    let tail = &rest[end_pos..];
    let body = tail
        .strip_prefix("\n---")
        .or_else(|| tail.strip_prefix("\r\n---"))
        .unwrap_or(tail);
    let body = body
        .strip_prefix('\n')
        .or_else(|| body.strip_prefix("\r\n"))
        .unwrap_or(body);
    Some((fm, body))
}

// frontmatter에서 `key: value` 한 줄을 찾아 값 반환.
fn fm_field(fm: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    fm.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .map(|v| v.trim().to_string())
}

// (description, body) — description은 단순 한 줄 파싱.
fn parse_frontmatter(content: &str) -> (Option<String>, &str) {
    match fm_block(content) {
        Some((fm, body)) => (fm_field(fm, "description"), body),
        None => (None, content),
    }
}

// 본문의 `$ARGUMENTS`를 인자 문자열로 치환. 인자 있는데 `$ARGUMENTS` 없으면 끝에 덧붙임.
fn expand_body(body: &str, args: &str) -> String {
    if body.contains("$ARGUMENTS") {
        body.replace("$ARGUMENTS", args)
    } else if !args.is_empty() {
        format!("{}\n\n{}", body, args)
    } else {
        body.to_string()
    }
}

// 이름 정규화: 소문자화, 허용 외 문자 → '-', 연속 '-' 압축, 앞뒤 '-' 제거.
fn normalize_name(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_dash = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            out.push(c);
            prev_dash = false;
        } else if c == '-' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

// frontmatter에서 description 파싱: 단일 줄, 따옴표 처리, >- folded scalar 지원.
fn parse_fm_description(fm: &str) -> Option<String> {
    let mut lines = fm.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(stripped) = line.strip_prefix("description:") else {
            continue;
        };
        let val = stripped.trim();
        if val == ">-" || val == ">" {
            // folded block: 이후 들여쓰기 줄들을 공백으로 join
            let mut parts = Vec::new();
            while let Some(next) = lines.peek() {
                if next.starts_with(' ') || next.starts_with('\t') {
                    parts.push(next.trim().to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            return Some(parts.join(" "));
        } else if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            return Some(val[1..val.len() - 1].to_string());
        } else {
            return Some(val.to_string());
        }
    }
    None
}

// claude SKILL.md / command .md → (description, body).
// parse_frontmatter의 description 파싱은 단순 strip_prefix만 하므로,
// >- folded scalar 처리를 위해 frontmatter 블록을 직접 재파싱.
fn parse_claude_md(content: &str) -> (String, String) {
    let (_, body) = parse_frontmatter(content);
    let desc = extract_fm_block(content)
        .and_then(parse_fm_description)
        .unwrap_or_default();
    let desc_line = desc.lines().collect::<Vec<_>>().join(" ");
    (desc_line, body.to_string())
}

// frontmatter 블록 텍스트(--- 사이)만 슬라이스로 반환.
fn extract_fm_block(content: &str) -> Option<&str> {
    let after = content.strip_prefix("---")?;
    let rest = after
        .strip_prefix('\n')
        .or_else(|| after.strip_prefix("\r\n"))?;
    let end_pos = rest.find("\n---").or_else(|| rest.find("\r\n---"))?;
    Some(&rest[..end_pos])
}

// TOML 수동 파서: prompt = '''...''' 또는 """...""", description = "..."
fn parse_toml_skill(content: &str) -> Option<(String, String)> {
    let mut description = String::new();
    let mut prompt = String::new();

    // description 한 줄 추출
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("description") {
            let rest = rest.trim_start_matches([' ', '\t']);
            if let Some(rest) = rest.strip_prefix('=') {
                let val = rest.trim();
                if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                    description = val[1..val.len() - 1].to_string();
                }
            }
            break;
        }
    }

    // prompt = '''...''' 또는 """...""" 멀티라인 추출
    let single = "'''";
    let double = "\"\"\"";
    for (start_delim, end_delim) in [(single, single), (double, double)] {
        let search = format!("prompt = {}", start_delim);
        if let Some(pos) = content.find(&search) {
            let after = &content[pos + search.len()..];
            // 시작 delim 직후 개행 제거
            let body_start = after
                .strip_prefix('\n')
                .or_else(|| after.strip_prefix("\r\n"))
                .unwrap_or(after);
            if let Some(end) = body_start.find(end_delim) {
                prompt = body_start[..end].trim_end_matches('\n').to_string();
                break;
            }
        }
    }

    if prompt.is_empty() {
        return None;
    }
    // {{args}} → $ARGUMENTS 치환
    let prompt = prompt.replace("{{args}}", "$ARGUMENTS");
    Some((description, prompt))
}

// ── 소스 ─────────────────────────────────────────────────────────────────────

/// 스킬이 사는 다섯 곳. 벤더 디렉터리이며 Praxis는 여기에 **쓰지 않는다**.
/// codex는 `~/.codex/skills/<name>/SKILL.md`(현행)와 `~/.codex/prompts/<name>.md`(구형) 둘 다 쓴다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    ClaudeSkills,
    ClaudeCommands,
    CodexSkills,
    CodexPrompts,
    GeminiCommands,
}

/// 목록 스캔 순서 — 앞선 것이 주 거처가 된다.
const ALL_SOURCES: [Source; 5] = [
    Source::ClaudeSkills,
    Source::ClaudeCommands,
    Source::CodexSkills,
    Source::CodexPrompts,
    Source::GeminiCommands,
];

impl Source {
    fn vendor(self) -> &'static str {
        match self {
            Source::ClaudeSkills | Source::ClaudeCommands => "claude",
            Source::CodexSkills | Source::CodexPrompts => "codex",
            Source::GeminiCommands => "antigravity",
        }
    }

    fn dir(self, base: &Path) -> PathBuf {
        match self {
            Source::ClaudeSkills => base.join(".claude").join("skills"),
            Source::ClaudeCommands => base.join(".claude").join("commands"),
            Source::CodexSkills => base.join(".codex").join("skills"),
            Source::CodexPrompts => base.join(".codex").join("prompts"),
            Source::GeminiCommands => base.join(".gemini").join("commands"),
        }
    }
}

/// 벤더별 탐색 순서 — 자기 것을 먼저 보고, 없으면 다른 벤더로 넘어간다(크로스 벤더 다리).
/// claude는 네이티브 확인이 앞서므로 여기에 claude 소스가 없다.
fn lookup_order(vendor: Vendor) -> &'static [Source] {
    match vendor {
        Vendor::Codex => &[
            Source::CodexSkills,
            Source::CodexPrompts,
            Source::ClaudeSkills,
            Source::ClaudeCommands,
            Source::GeminiCommands,
        ],
        Vendor::Agy => &[
            Source::GeminiCommands,
            Source::ClaudeSkills,
            Source::ClaudeCommands,
            Source::CodexSkills,
            Source::CodexPrompts,
        ],
        Vendor::Claude => &[
            Source::CodexSkills,
            Source::CodexPrompts,
            Source::GeminiCommands,
        ],
    }
}

// ── 스캔 ─────────────────────────────────────────────────────────────────────

// 스캔 한 건 — 이름 하나가 한 곳에서 발견된 결과.
struct Found {
    name: String,
    description: String,
    bytes: u64,
    argument_hint: String,
    home: SkillHome,
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

// claude 계열 파일 하나를 Found로. `.md` frontmatter에서 description·argument-hint를 읽는다.
fn found_from_claude_file(path: &Path, name: String, project: bool, vendor: &str) -> Option<Found> {
    if path.metadata().map(|m| m.len()).unwrap_or(0) > MAX_SKILL_BYTES {
        return None;
    }
    let content = read_counted(path)?;
    let (desc, body) = parse_claude_md(&content);
    let hint = extract_fm_block(&content)
        .and_then(|fm| fm_field(fm, "argument-hint"))
        .unwrap_or_default();
    Some(Found {
        name,
        description: desc,
        bytes: body.len() as u64,
        argument_hint: hint,
        home: SkillHome {
            vendor: vendor.to_string(),
            path: path_str(path),
            project,
        },
    })
}

// `<dir>/<name>/SKILL.md` 스캔 — claude skills와 codex skills가 같은 모양이다.
fn scan_skill_dirs(dir: &Path, project: bool, vendor: &str) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }
        let Some(stem) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = normalize_name(stem);
        if !is_valid_name(&name) {
            continue;
        }
        if let Some(f) = found_from_claude_file(&skill_file, name, project, vendor) {
            out.push(f);
        }
    }
    out
}

// `<dir>/<name>.md` 스캔 — claude commands와 codex prompts가 같은 모양이다.
fn scan_md_dir(dir: &Path, project: bool, vendor: &str) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "md").unwrap_or(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = normalize_name(stem);
        if !is_valid_name(&name) {
            continue;
        }
        if let Some(f) = found_from_claude_file(&path, name, project, vendor) {
            out.push(f);
        }
    }
    out
}

// TOML 하나를 Found로.
fn found_from_toml(path: &Path, name: String, project: bool) -> Option<Found> {
    if path.metadata().map(|m| m.len()).unwrap_or(0) > MAX_SKILL_BYTES {
        return None;
    }
    let content = read_counted(path)?;
    let (desc, body) = parse_toml_skill(&content)?;
    if body.len() as u64 > MAX_SKILL_BYTES {
        return None;
    }
    Some(Found {
        name,
        description: desc,
        bytes: body.len() as u64,
        argument_hint: String::new(),
        home: SkillHome {
            vendor: "antigravity".to_string(),
            path: path_str(path),
            project,
        },
    })
}

// `~/.gemini/commands/**/*.toml` — 1단계 네임스페이스는 `<ns>-<name>`으로 접는다.
fn scan_gemini(dir: &Path, project: bool) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let Some(ns) = path.file_name().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            let Ok(sub) = std::fs::read_dir(&path) else {
                continue;
            };
            for sub_entry in sub.flatten() {
                let sub_path = sub_entry.path();
                if sub_path.extension().map(|e| e != "toml").unwrap_or(true) {
                    continue;
                }
                let Some(stem) = sub_path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let name = normalize_name(&format!("{ns}-{stem}"));
                if !is_valid_name(&name) {
                    continue;
                }
                if let Some(f) = found_from_toml(&sub_path, name, project) {
                    out.push(f);
                }
            }
        } else if path.extension().map(|e| e == "toml").unwrap_or(false) {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let name = normalize_name(stem);
            if !is_valid_name(&name) {
                continue;
            }
            if let Some(f) = found_from_toml(&path, name, project) {
                out.push(f);
            }
        }
    }
    out
}

fn scan_source(src: Source, base: &Path, project: bool) -> Vec<Found> {
    let dir = src.dir(base);
    match src {
        Source::ClaudeSkills | Source::CodexSkills => scan_skill_dirs(&dir, project, src.vendor()),
        Source::ClaudeCommands | Source::CodexPrompts => scan_md_dir(&dir, project, src.vendor()),
        Source::GeminiCommands => scan_gemini(&dir, project),
    }
}

// ── 플러그인 ─────────────────────────────────────────────────────────────────

// 설치된 플러그인의 매니페스트. `marketplaces/` 아래를 직접 훑으면 설치하지 않은
// 카탈로그 전체가 딸려오므로, 무엇이 실제로 깔렸는지는 이 파일만이 안다.
fn plugin_manifest(base: &Path) -> PathBuf {
    base.join(".claude")
        .join("plugins")
        .join("installed_plugins.json")
}

// (플러그인 이름, 설치 경로) 목록. 키는 `<plugin>@<marketplace>` — 슬래시 이름이 되는
// 것은 `@` 앞부분이다.
fn plugin_installs(base: &Path) -> Vec<(String, PathBuf)> {
    let manifest = plugin_manifest(base);
    if manifest.metadata().map(|m| m.len()).unwrap_or(0) > MAX_SKILL_BYTES {
        return vec![];
    }
    let Some(raw) = read_counted(&manifest) else {
        return vec![];
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return vec![];
    };
    let Some(map) = json.get("plugins").and_then(|v| v.as_object()) else {
        return vec![];
    };
    let mut out = Vec::new();
    for (key, entries) in map {
        let name = normalize_name(key.split('@').next().unwrap_or(key));
        if !is_valid_name(&name) {
            continue;
        }
        let Some(list) = entries.as_array() else {
            continue;
        };
        for entry in list {
            if let Some(path) = entry.get("installPath").and_then(|v| v.as_str()) {
                out.push((name.clone(), PathBuf::from(path)));
            }
        }
    }
    out.sort();
    out
}

// 설치된 플러그인이 담은 스킬·커맨드. 이름은 claude CLI가 쓰는 `<plugin>:<name>` 그대로다 —
// `-`로 접으면 그 이름으로는 CLI가 발동하지 못한다.
fn scan_plugins(base: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    for (plugin, root) in plugin_installs(base) {
        let skills = scan_skill_dirs(&root.join("skills"), false, "claude");
        let commands = scan_md_dir(&root.join("commands"), false, "claude");
        for mut f in skills.into_iter().chain(commands) {
            f.name = format!("{plugin}:{}", f.name);
            out.push(f);
        }
    }
    out
}

// `<plugin>:<name>` → 실제 파일. 스킬이 먼저, 없으면 커맨드.
fn plugin_skill_path(base: &Path, name: &str) -> Option<PathBuf> {
    let (plugin, rest) = name.split_once(':')?;
    if !is_valid_name(plugin) || !is_valid_name(rest) {
        return None;
    }
    plugin_installs(base)
        .into_iter()
        .filter(|(installed, _)| installed == plugin)
        .find_map(|(_, root)| {
            let skill = root.join("skills").join(rest).join("SKILL.md");
            if skill.exists() {
                return Some(skill);
            }
            let command = root.join("commands").join(format!("{rest}.md"));
            command.exists().then_some(command)
        })
}

// 남은 Praxis 사본 — 벤더 어디에도 없는 이름만 고아로 실린다.
fn scan_praxis_orphans(base: &Path, known: &std::collections::HashSet<String>) -> Vec<SkillMeta> {
    let dir = base.join(".praxis").join("skills");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e != "md").unwrap_or(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = normalize_name(stem);
        if !is_valid_name(&name) || known.contains(&name) {
            continue;
        }
        if path.metadata().map(|m| m.len()).unwrap_or(0) > MAX_SKILL_BYTES {
            continue;
        }
        let Some(content) = read_counted(&path) else {
            continue;
        };
        let (desc, body) = parse_claude_md(&content);
        out.push(SkillMeta {
            name,
            description: desc,
            global: true,
            source: "praxis".to_string(),
            homes: vec![],
            bytes: body.len() as u64,
            argument_hint: String::new(),
            orphan: true,
        });
    }
    out
}

/// 벤더 디렉터리 실측 — 프로젝트 레이어가 글로벌보다 앞선다.
/// 같은 이름이 여러 곳에 있으면 한 행으로 접고 거처를 `homes`에 모은다.
pub fn collect_skills(repo: &str, home: &Path) -> Vec<SkillMeta> {
    let mut bases: Vec<(PathBuf, bool)> = Vec::new();
    if !repo.is_empty() {
        bases.push((PathBuf::from(repo), true));
    }
    bases.push((home.to_path_buf(), false));

    let mut order: Vec<String> = Vec::new();
    let mut by_name: std::collections::HashMap<String, SkillMeta> =
        std::collections::HashMap::new();

    for (base, project) in &bases {
        let mut found: Vec<Found> = Vec::new();
        for src in ALL_SOURCES {
            found.extend(scan_source(src, base, *project));
        }
        // 플러그인은 사용자 스코프에 설치된다 — 프로젝트 레이어에는 없다.
        if !*project {
            found.extend(scan_plugins(base));
        }
        for f in found {
            match by_name.get_mut(&f.name) {
                Some(existing) => existing.homes.push(f.home),
                None => {
                    order.push(f.name.clone());
                    by_name.insert(
                        f.name.clone(),
                        SkillMeta {
                            name: f.name,
                            description: f.description,
                            global: false, // 아래에서 homes를 보고 확정
                            source: f.home.vendor.clone(),
                            homes: vec![f.home],
                            bytes: f.bytes,
                            argument_hint: f.argument_hint,
                            orphan: false,
                        },
                    );
                }
            }
        }
    }

    let mut all: Vec<SkillMeta> = order
        .into_iter()
        .filter_map(|n| by_name.remove(&n))
        .map(|mut m| {
            m.global = m.homes.iter().all(|h| !h.project);
            m
        })
        .collect();

    let known: std::collections::HashSet<String> = all.iter().map(|m| m.name.clone()).collect();
    for (base, _) in &bases {
        for orphan in scan_praxis_orphans(base, &known) {
            if !all.iter().any(|m| m.name == orphan.name) {
                all.push(orphan);
            }
        }
    }

    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

// ── 캐시 ─────────────────────────────────────────────────────────────────────

type DirStamp = Vec<(PathBuf, Option<std::time::SystemTime>)>;

struct ScanCache {
    repo: String,
    stamp: DirStamp,
    result: Vec<SkillMeta>,
}

static CACHE: OnceLock<Mutex<Option<ScanCache>>> = OnceLock::new();

// 스캔이 훑는 디렉터리들의 mtime — 하나라도 바뀌면 캐시를 버린다.
fn dir_stamp(repo: &str, home: &Path) -> DirStamp {
    let mut bases: Vec<PathBuf> = Vec::new();
    if !repo.is_empty() {
        bases.push(PathBuf::from(repo));
    }
    bases.push(home.to_path_buf());

    let mut out = Vec::new();
    for base in &bases {
        for src in ALL_SOURCES {
            let dir = src.dir(base);
            let mtime = std::fs::metadata(&dir).and_then(|m| m.modified()).ok();
            out.push((dir, mtime));
        }
        let praxis = base.join(".praxis").join("skills");
        let mtime = std::fs::metadata(&praxis).and_then(|m| m.modified()).ok();
        out.push((praxis, mtime));
        // 설치·삭제는 이 매니페스트를 다시 쓴다. 파일을 열지 않고 mtime만 본다 —
        // 스탬프는 캐시 히트마다 계산되므로 여기서 읽으면 캐시가 무의미해진다.
        let manifest = plugin_manifest(base);
        let mtime = std::fs::metadata(&manifest).and_then(|m| m.modified()).ok();
        out.push((manifest, mtime));
    }
    out
}

/// 스킬 목록 — 디렉터리 mtime이 그대로면 직전 결과를 재사용한다.
pub fn list_skills_cached(repo: &str, home: &Path) -> Vec<SkillMeta> {
    let stamp = dir_stamp(repo, home);
    let cell = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = guard.as_ref() {
        if cached.repo == repo && cached.stamp == stamp {
            return cached.result.clone();
        }
    }
    let result = collect_skills(repo, home);
    *guard = Some(ScanCache {
        repo: repo.to_string(),
        stamp,
        result: result.clone(),
    });
    result
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

/// 스킬 목록 — 벤더 디렉터리 실측. Praxis 사본은 고아로만 실린다.
pub fn list_skills(repo: &str) -> Vec<SkillMeta> {
    match home_dir() {
        Some(home) => list_skills_cached(repo, &home),
        None => vec![],
    }
}

// ── 발동 ─────────────────────────────────────────────────────────────────────

/// claude가 스스로 해석할 수 있는가 — 디렉터리 스캔이 아니라 경로 직접 stat 4회.
pub fn claude_resolves_natively(repo: &str, name: &str, home: &Path) -> bool {
    // 플러그인 스킬은 `<plugin>:<name>`으로 claude가 스스로 찾는다.
    if name.contains(':') {
        return plugin_skill_path(home, name).is_some();
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if !repo.is_empty() {
        bases.push(PathBuf::from(repo));
    }
    bases.push(home.to_path_buf());

    bases.iter().any(|base| {
        base.join(".claude")
            .join("skills")
            .join(name)
            .join("SKILL.md")
            .exists()
            || base
                .join(".claude")
                .join("commands")
                .join(format!("{name}.md"))
                .exists()
    })
}

// gemini는 `<ns>-<name>`으로 접혀 있어 역추적이 필요하다.
fn gemini_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let flat = dir.join(format!("{name}.toml"));
    if flat.exists() {
        return Some(flat);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let ns = path.file_name().and_then(|s| s.to_str())?.to_string();
        let sub = std::fs::read_dir(&path).ok()?;
        for sub_entry in sub.flatten() {
            let sub_path = sub_entry.path();
            if sub_path.extension().map(|e| e != "toml").unwrap_or(true) {
                continue;
            }
            let stem = sub_path.file_stem().and_then(|s| s.to_str())?.to_string();
            if normalize_name(&format!("{ns}-{stem}")) == name {
                return Some(sub_path);
            }
        }
    }
    None
}

// 한 소스에서 본문을 읽는다. 없으면 None.
fn load_body(src: Source, base: &Path, name: &str) -> Option<String> {
    let dir = src.dir(base);
    match src {
        Source::ClaudeSkills | Source::CodexSkills => {
            let path = dir.join(name).join("SKILL.md");
            let content = read_counted(&path)?;
            Some(parse_claude_md(&content).1)
        }
        Source::ClaudeCommands | Source::CodexPrompts => {
            let path = dir.join(format!("{name}.md"));
            let content = read_counted(&path)?;
            Some(parse_claude_md(&content).1)
        }
        Source::GeminiCommands => {
            let path = gemini_path(&dir, name)?;
            let content = read_counted(&path)?;
            parse_toml_skill(&content).map(|(_, body)| body)
        }
    }
}

/// 슬래시 스킬 발동 — 대상이 스스로 해석하면 `None`(원문 그대로 전송),
/// 못 하면 그 스킬이 사는 디렉터리에서 읽어 확장한다. **사본을 만들지 않는다.**
pub fn resolve_message(repo: &str, vendor: Vendor, message: &str) -> Option<String> {
    let home = home_dir()?;
    resolve_message_with_home(repo, vendor, message, &home)
}

/// 홈 주입판 — 테스트용.
pub fn resolve_message_with_home(
    repo: &str,
    vendor: Vendor,
    message: &str,
    home: &Path,
) -> Option<String> {
    let body = message.strip_prefix('/')?;
    // 첫 공백 전까지가 스킬 이름
    let (name, raw_args) = match body.find(|c: char| c.is_whitespace()) {
        Some(pos) => (&body[..pos], body[pos + 1..].trim()),
        None => (body.trim(), ""),
    };
    if !is_valid_skill_ref(name) {
        return None;
    }
    // 대상이 해석하면 손대지 않는다 — 확장은 순수 열화다(토큰·allowed-tools·귀속).
    if vendor == Vendor::Claude && claude_resolves_natively(repo, name, home) {
        return None;
    }

    let mut bases: Vec<PathBuf> = Vec::new();
    if !repo.is_empty() {
        bases.push(PathBuf::from(repo));
    }
    bases.push(home.to_path_buf());

    for src in lookup_order(vendor) {
        for base in &bases {
            if let Some(body) = load_body(*src, base, name) {
                return Some(expand_body(&body, raw_args));
            }
        }
    }

    // 플러그인 스킬 — claude가 아닌 벤더는 이 이름을 모르므로 읽어서 확장한다.
    if let Some(path) = plugin_skill_path(home, name) {
        if let Some(content) = read_counted(&path) {
            return Some(expand_body(&parse_claude_md(&content).1, raw_args));
        }
    }

    // 하위호환 — 아직 옮겨지지 않은 Praxis 사본. 탐색의 **맨 끝**이라 벤더 원본을 가리지 않는다.
    // 고아가 마이그레이션되기 전에 죽지 않게 하려는 것뿐이고, 다음 릴리스에서 뗀다(설계 0052 §D).
    for base in &bases {
        let path = base
            .join(".praxis")
            .join("skills")
            .join(format!("{name}.md"));
        if let Some(content) = read_counted(&path) {
            return Some(expand_body(&parse_claude_md(&content).1, raw_args));
        }
    }
    None
}

/// 스킬 본문 — 벤더 디렉터리에서 읽는다. 목록의 주 거처를 그대로 따른다.
pub fn read_skill(repo: &str, name: &str) -> Result<String, String> {
    if !is_valid_skill_ref(name) {
        return Err(format!(
            "유효하지 않은 스킬 이름: '{name}' (소문자, 숫자, -, _ 와 플러그인 구분자 : 만 허용)"
        ));
    }
    let home = home_dir().ok_or_else(|| "홈 디렉터리를 확인할 수 없습니다".to_string())?;
    let meta = list_skills(repo).into_iter().find(|m| m.name == name);
    let path = match meta.as_ref().and_then(|m| m.homes.first()) {
        Some(h) => PathBuf::from(&h.path),
        None => home
            .join(".praxis")
            .join("skills")
            .join(format!("{name}.md")),
    };
    if !path.exists() {
        return Err(format!("스킬을 찾을 수 없습니다: '{name}'"));
    }
    if path.metadata().map(|m| m.len()).unwrap_or(0) > MAX_SKILL_BYTES {
        return Err(format!(
            "스킬 파일이 {}KB 한도를 초과합니다: '{name}'",
            MAX_SKILL_BYTES / 1024
        ));
    }
    std::fs::read_to_string(&path).map_err(|e| format!("파일 읽기 실패: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicU32;
    use std::sync::MutexGuard;

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    // 파일을 읽는 테스트는 전부 직렬화한다 — FILE_READS는 전역 카운터라
    // 병렬 실행이면 캐시 테스트의 델타가 오염된다.
    fn guard() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tmp_home(prefix: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = crate::testtmp::dir().join(format!(
            "praxis-skills-{}-{}-{}",
            std::process::id(),
            n,
            prefix
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
            base.join(".claude").join("skills").join(name).join("SKILL.md"),
            content,
        );
    }

    fn claude_command(base: &Path, name: &str, content: &str) {
        write_at(
            base.join(".claude").join("commands").join(format!("{name}.md")),
            content,
        );
    }

    fn codex_prompt(base: &Path, name: &str, content: &str) {
        write_at(
            base.join(".codex").join("prompts").join(format!("{name}.md")),
            content,
        );
    }

    fn codex_skill(base: &Path, name: &str, content: &str) {
        write_at(
            base.join(".codex")
                .join("skills")
                .join(name)
                .join("SKILL.md"),
            content,
        );
    }

    fn gemini_command(base: &Path, ns: &str, name: &str, content: &str) {
        write_at(
            base.join(".gemini").join("commands").join(ns).join(format!("{name}.toml")),
            content,
        );
    }

    // 설치된 플러그인 하나를 흉내낸다 — 매니페스트가 없으면 스캔 대상이 아니다.
    fn plugin_install(base: &Path, plugin: &str, marketplace: &str) -> PathBuf {
        let root = base
            .join(".claude")
            .join("plugins")
            .join("cache")
            .join(marketplace)
            .join(plugin);
        let manifest = format!(
            r#"{{"version":2,"plugins":{{"{plugin}@{marketplace}":[{{"scope":"user","installPath":"{}"}}]}}}}"#,
            root.to_string_lossy()
        );
        write_at(
            base.join(".claude")
                .join("plugins")
                .join("installed_plugins.json"),
            &manifest,
        );
        root
    }

    fn plugin_skill(root: &Path, name: &str, content: &str) {
        write_at(root.join("skills").join(name).join("SKILL.md"), content);
    }

    fn plugin_command(root: &Path, name: &str, content: &str) {
        write_at(root.join("commands").join(format!("{name}.md")), content);
    }

    fn praxis_copy(base: &Path, name: &str, content: &str) {
        write_at(
            base.join(".praxis").join("skills").join(format!("{name}.md")),
            content,
        );
    }

    // ── 파서 ──

    #[test]
    fn frontmatter_parsed_and_description_extracted() {
        let content = "---\ndescription: 코드 리뷰\n---\n리뷰 본문";
        let (desc, body) = parse_frontmatter(content);
        assert_eq!(desc.as_deref(), Some("코드 리뷰"));
        assert_eq!(body, "리뷰 본문");
    }

    #[test]
    fn no_frontmatter_returns_full_content_as_body() {
        let content = "그냥 본문";
        let (desc, body) = parse_frontmatter(content);
        assert!(desc.is_none());
        assert_eq!(body, "그냥 본문");
    }

    #[test]
    fn arguments_substituted_in_body() {
        assert_eq!(expand_body("리뷰: $ARGUMENTS", "my_func"), "리뷰: my_func");
    }

    #[test]
    fn arguments_appended_when_no_placeholder() {
        assert_eq!(expand_body("본문", "내 코드"), "본문\n\n내 코드");
    }

    #[test]
    fn no_arguments_placeholder_replaced_with_empty() {
        assert_eq!(expand_body("리뷰: $ARGUMENTS", ""), "리뷰: ");
    }

    #[test]
    fn normalize_name_lowercases_and_replaces_invalid() {
        assert_eq!(normalize_name("My Skill!"), "my-skill");
        assert_eq!(normalize_name("A--B"), "a-b");
    }

    #[test]
    fn parse_fm_description_handles_folded_multiline() {
        let fm = "name: x\ndescription: >-\n  첫 줄\n  둘째 줄\nmodel: y";
        assert_eq!(
            parse_fm_description(fm).as_deref(),
            Some("첫 줄 둘째 줄")
        );
    }

    #[test]
    fn parse_fm_description_handles_quoted() {
        assert_eq!(
            parse_fm_description("description: \"따옴표\"").as_deref(),
            Some("따옴표")
        );
    }

    #[test]
    fn parse_fm_description_handles_plain() {
        assert_eq!(
            parse_fm_description("description: 평문").as_deref(),
            Some("평문")
        );
    }

    #[test]
    fn antigravity_toml_parsed_with_args_substitution() {
        let toml = "description = \"설명\"\nprompt = '''\n본문 {{args}}\n'''";
        let (desc, body) = parse_toml_skill(toml).unwrap();
        assert_eq!(desc, "설명");
        assert_eq!(body, "본문 $ARGUMENTS");
    }

    // ── Task 1 · 거처 ──

    #[test]
    fn skill_meta_carries_multiple_homes() {
        let _g = guard();
        let home = tmp_home("homes");
        claude_skill(&home, "foo", "---\ndescription: c\n---\nclaude 본문");
        codex_prompt(&home, "foo", "---\ndescription: x\n---\ncodex 본문");

        let list = collect_skills("", &home);
        let foo = list.iter().find(|m| m.name == "foo").unwrap();
        assert_eq!(foo.homes.len(), 2, "거처가 둘이어야 한다: {:?}", foo.homes);
        assert_eq!(foo.source, "claude", "주 거처는 스캔 순서상 claude");
        assert!(foo.global);
        assert!(!foo.orphan);
    }

    #[test]
    fn project_layer_precedes_global() {
        let _g = guard();
        let home = tmp_home("proj-home");
        let repo = tmp_home("proj-repo");
        claude_skill(&home, "bar", "---\ndescription: 글로벌\n---\n글로벌 본문");
        claude_skill(&repo, "bar", "---\ndescription: 프로젝트\n---\n프로젝트 본문");

        let list = collect_skills(repo.to_str().unwrap(), &home);
        let bar = list.iter().find(|m| m.name == "bar").unwrap();
        assert_eq!(bar.description, "프로젝트", "프로젝트가 주 거처");
        assert!(bar.homes[0].project);
        assert!(!bar.global, "프로젝트 거처가 있으면 global이 아니다");
    }

    // ── Task 2 · 통합 스캔 ──

    #[test]
    fn list_skills_reads_vendor_dirs_not_praxis_copies() {
        let _g = guard();
        let home = tmp_home("vendor-scan");
        claude_skill(&home, "alpha", "---\ndescription: A\n---\n본문 A");
        codex_prompt(&home, "beta", "---\ndescription: B\n---\n본문 B");
        praxis_copy(&home, "gamma", "---\ndescription: G\n---\n본문 G");

        let list = collect_skills("", &home);
        assert_eq!(list.len(), 3, "{:?}", list.iter().map(|m| &m.name).collect::<Vec<_>>());

        let gamma = list.iter().find(|m| m.name == "gamma").unwrap();
        assert!(gamma.orphan, "벤더에 없는 사본만 고아");
        assert!(gamma.homes.is_empty(), "고아는 거처가 없다");

        for n in ["alpha", "beta"] {
            let m = list.iter().find(|m| m.name == n).unwrap();
            assert!(!m.orphan, "{n}은 벤더 소유라 고아가 아니다");
        }
    }

    #[test]
    fn praxis_copy_shadowed_by_vendor_original_is_not_listed_twice() {
        let _g = guard();
        let home = tmp_home("shadow");
        claude_skill(&home, "dup", "---\ndescription: 최신\n---\n벤더 본문");
        praxis_copy(&home, "dup", "---\ndescription: 옛것\n---\n사본 본문");

        let list = collect_skills("", &home);
        let dups: Vec<_> = list.iter().filter(|m| m.name == "dup").collect();
        assert_eq!(dups.len(), 1, "한 행으로 접혀야 한다");
        assert_eq!(dups[0].description, "최신", "벤더 원본이 이긴다");
        assert!(!dups[0].orphan);
    }

    #[test]
    fn metadata_carries_bytes_and_argument_hint() {
        let _g = guard();
        let home = tmp_home("meta");
        claude_skill(
            &home,
            "hinted",
            "---\ndescription: D\nargument-hint: <파일>\n---\n0123456789",
        );
        let list = collect_skills("", &home);
        let m = list.iter().find(|m| m.name == "hinted").unwrap();
        assert_eq!(m.argument_hint, "<파일>");
        assert_eq!(m.bytes, 10, "본문만 센다(frontmatter 제외)");
    }

    #[test]
    fn invalid_names_and_oversized_files_are_skipped() {
        let _g = guard();
        let home = tmp_home("guards");
        claude_skill(&home, "GOOD", "---\ndescription: d\n---\n본문"); // 정규화되어 살아남음
        let big = "x".repeat(130 * 1024);
        claude_skill(&home, "toobig", &format!("---\ndescription: d\n---\n{big}"));
        let mid = "x".repeat(70 * 1024);
        claude_skill(&home, "chunky", &format!("---\ndescription: d\n---\n{mid}"));

        let list = collect_skills("", &home);
        assert!(list.iter().any(|m| m.name == "good"), "대문자는 정규화된다");
        assert!(!list.iter().any(|m| m.name == "toobig"), "상한 초과는 제외");
        assert!(
            list.iter().any(|m| m.name == "chunky"),
            "상한 안이면 커도 목록에 남는다 — 여기서 좁히면 존재하는 스킬이 사라진다"
        );
    }

    // ── 플러그인 ──

    #[test]
    fn plugin_skills_and_commands_are_listed_under_plugin_prefix() {
        let _g = guard();
        let home = tmp_home("plugin-list");
        let root = plugin_install(&home, "codex", "openai-codex");
        plugin_skill(&root, "rescue", "---\ndescription: 구조\n---\n본문");
        plugin_command(&root, "review", "---\ndescription: 리뷰\n---\n본문");

        let list = collect_skills("", &home);
        let rescue = list.iter().find(|m| m.name == "codex:rescue").unwrap();
        assert_eq!(rescue.description, "구조");
        assert_eq!(rescue.source, "claude", "발동은 claude CLI가 한다");
        assert!(rescue.global);
        assert!(list.iter().any(|m| m.name == "codex:review"));
    }

    #[test]
    fn uninstalled_marketplace_plugins_are_not_listed() {
        let _g = guard();
        let home = tmp_home("plugin-catalog");
        // 매니페스트에 없는 카탈로그 사본 — 설치되지 않은 것은 목록에 오르면 안 된다.
        let stray = home
            .join(".claude")
            .join("plugins")
            .join("marketplaces")
            .join("official")
            .join("plugins")
            .join("ghost");
        plugin_command(&stray, "haunt", "---\ndescription: d\n---\n본문");

        let list = collect_skills("", &home);
        assert!(!list.iter().any(|m| m.name.contains("haunt")));
    }

    #[test]
    fn claude_resolves_plugin_skill_natively() {
        let _g = guard();
        let home = tmp_home("plugin-native");
        let root = plugin_install(&home, "codex", "openai-codex");
        plugin_skill(&root, "rescue", "---\ndescription: d\n---\n확장하면 안 되는 본문");

        assert!(claude_resolves_natively("", "codex:rescue", &home));
        assert_eq!(
            resolve_message_with_home("", Vendor::Claude, "/codex:rescue", &home),
            None,
            "claude는 스스로 해석한다 — 확장은 순수 열화다"
        );
    }

    #[test]
    fn other_vendors_expand_plugin_skill_body() {
        let _g = guard();
        let home = tmp_home("plugin-bridge");
        let root = plugin_install(&home, "codex", "openai-codex");
        plugin_command(&root, "review", "---\ndescription: d\n---\n리뷰 지침 $ARGUMENTS");

        assert_eq!(
            resolve_message_with_home("", Vendor::Agy, "/codex:review src/", &home),
            Some("리뷰 지침 src/".to_string()),
            "플러그인을 모르는 벤더에는 본문을 읽어 넘긴다"
        );
    }

    #[test]
    fn plugin_name_must_be_two_valid_halves() {
        assert!(is_valid_skill_ref("feature-development"));
        assert!(is_valid_skill_ref("codex:rescue"));
        assert!(!is_valid_skill_ref("codex:"));
        assert!(!is_valid_skill_ref(":rescue"));
        assert!(!is_valid_skill_ref("a:b:c"), "구분자는 하나뿐이다");
        assert!(!is_valid_skill_ref("../etc/passwd"));
    }

    // ── Task 3 · 캐시 ──

    #[test]
    fn scan_is_skipped_when_dirs_unchanged() {
        let _g = guard();
        let home = tmp_home("cache");
        claude_skill(&home, "cached", "---\ndescription: d\n---\n본문");

        let first = list_skills_cached("", &home);
        let after_first = file_read_count();
        let second = list_skills_cached("", &home);
        let after_second = file_read_count();

        assert_eq!(first.len(), second.len());
        assert_eq!(
            after_first, after_second,
            "mtime이 그대로면 파일을 다시 열지 않는다"
        );
    }

    // ── Task 4 · 네이티브 판별 ──

    #[test]
    fn claude_native_lookup_finds_skills_and_commands() {
        let _g = guard();
        let home = tmp_home("native");
        claude_skill(&home, "viaskill", "---\ndescription: d\n---\n본문");
        claude_command(&home, "viacmd", "---\ndescription: d\n---\n본문");

        assert!(claude_resolves_natively("", "viaskill", &home));
        assert!(claude_resolves_natively("", "viacmd", &home));
        assert!(!claude_resolves_natively("", "nope", &home));
    }

    #[test]
    fn claude_native_lookup_sees_project_layer() {
        let _g = guard();
        let home = tmp_home("native-home");
        let repo = tmp_home("native-repo");
        claude_skill(&repo, "projonly", "---\ndescription: d\n---\n본문");
        assert!(claude_resolves_natively(repo.to_str().unwrap(), "projonly", &home));
        assert!(!claude_resolves_natively("", "projonly", &home));
    }

    // ── Task 5 · 탐색 순서 ──

    #[test]
    fn codex_prefers_own_prompts() {
        let _g = guard();
        let home = tmp_home("codex-order");
        codex_prompt(&home, "shared", "---\ndescription: d\n---\ncodex 본문");
        claude_skill(&home, "shared", "---\ndescription: d\n---\nclaude 본문");

        let out = resolve_message_with_home("", Vendor::Codex, "/shared", &home).unwrap();
        assert_eq!(out, "codex 본문");
    }

    #[test]
    fn codex_skills_dir_is_listed_and_preferred_over_prompts() {
        let _g = guard();
        let home = tmp_home("codex-skills");
        codex_skill(&home, "shared", "---\ndescription: 현행\n---\nskills 본문");
        codex_prompt(&home, "shared", "---\ndescription: 구형\n---\nprompts 본문");
        codex_skill(&home, "onlyskill", "---\ndescription: d\n---\n스킬만");

        let list = collect_skills("", &home);
        let shared = list.iter().find(|m| m.name == "shared").expect("shared 목록");
        assert_eq!(shared.source, "codex");
        assert_eq!(shared.homes.len(), 2, "skills·prompts 두 거처");
        assert!(shared.homes[0].path.ends_with("skills/shared/SKILL.md"), "주 거처는 skills");
        let only = list.iter().find(|m| m.name == "onlyskill").expect("onlyskill 목록");
        assert!(only.homes.iter().all(|h| h.vendor == "codex"));

        let out = resolve_message_with_home("", Vendor::Codex, "/shared", &home).unwrap();
        assert_eq!(out, "skills 본문");
        let out = resolve_message_with_home("", Vendor::Claude, "/onlyskill", &home).unwrap();
        assert_eq!(out, "스킬만", "claude는 codex skills를 다리로 읽는다");
    }

    #[test]
    fn agy_prefers_own_commands() {
        let _g = guard();
        let home = tmp_home("agy-order");
        gemini_command(&home, "ns", "shared", "description = \"d\"\nprompt = '''\nagy 본문\n'''");
        claude_skill(&home, "ns-shared", "---\ndescription: d\n---\nclaude 본문");

        let out = resolve_message_with_home("", Vendor::Agy, "/ns-shared", &home).unwrap();
        assert_eq!(out, "agy 본문");
    }

    #[test]
    fn cross_vendor_bridge_falls_back_to_claude() {
        let _g = guard();
        let home = tmp_home("bridge");
        claude_skill(&home, "onlyclaude", "---\ndescription: d\n---\nclaude 본문");

        let out = resolve_message_with_home("", Vendor::Codex, "/onlyclaude", &home).unwrap();
        assert_eq!(out, "claude 본문", "codex는 claude 것을 읽어 확장한다");
    }

    // ── Task 6 · 발동 ──

    #[test]
    fn claude_with_native_skill_is_not_expanded() {
        let _g = guard();
        let home = tmp_home("no-expand");
        claude_skill(&home, "native", "---\ndescription: d\n---\n본문");

        assert!(
            resolve_message_with_home("", Vendor::Claude, "/native", &home).is_none(),
            "claude가 해석하므로 손대지 않는다"
        );
    }

    #[test]
    fn codex_with_claude_skill_is_expanded_to_exact_body() {
        let _g = guard();
        let home = tmp_home("expand-exact");
        let body = "1단계\n2단계";
        claude_skill(&home, "steps", &format!("---\ndescription: d\n---\n{body}"));

        let out = resolve_message_with_home("", Vendor::Codex, "/steps", &home).unwrap();
        assert_eq!(out, body, "본문과 바이트 일치");
    }

    #[test]
    fn arguments_are_substituted_on_expansion() {
        let _g = guard();
        let home = tmp_home("expand-args");
        claude_skill(&home, "withargs", "---\ndescription: d\n---\n대상: $ARGUMENTS");

        let out = resolve_message_with_home("", Vendor::Codex, "/withargs src/a.ts", &home).unwrap();
        assert_eq!(out, "대상: src/a.ts");
    }

    #[test]
    fn unknown_and_plain_messages_pass_through() {
        let _g = guard();
        let home = tmp_home("passthrough");
        assert!(resolve_message_with_home("", Vendor::Codex, "/nosuch", &home).is_none());
        assert!(resolve_message_with_home("", Vendor::Codex, "일반 텍스트", &home).is_none());
        assert!(resolve_message_with_home("", Vendor::Codex, "/BAD-NAME", &home).is_none());
    }

    #[test]
    fn claude_without_native_skill_bridges_to_other_vendor() {
        let _g = guard();
        let home = tmp_home("claude-bridge");
        codex_prompt(&home, "codexonly", "---\ndescription: d\n---\ncodex 본문");

        let out = resolve_message_with_home("", Vendor::Claude, "/codexonly", &home).unwrap();
        assert_eq!(out, "codex 본문", "claude가 못 가진 것은 다리를 놓는다");
    }

    #[test]
    fn orphan_praxis_copy_still_fires_until_migrated() {
        let _g = guard();
        let home = tmp_home("orphan-fire");
        praxis_copy(&home, "stranded", "---\ndescription: d\n---\n사본 본문");

        let out = resolve_message_with_home("", Vendor::Codex, "/stranded", &home).unwrap();
        assert_eq!(out, "사본 본문", "옮겨지기 전에는 사본이라도 발동한다");
    }

    #[test]
    fn vendor_original_outranks_the_praxis_copy() {
        let _g = guard();
        let home = tmp_home("orphan-rank");
        claude_skill(&home, "both", "---\ndescription: d\n---\n벤더 본문");
        praxis_copy(&home, "both", "---\ndescription: d\n---\n사본 본문");

        let out = resolve_message_with_home("", Vendor::Codex, "/both", &home).unwrap();
        assert_eq!(out, "벤더 본문", "사본은 탐색의 맨 끝이라 원본을 가리지 못한다");
    }

    // ── Task 12 · 쓰기 금지 ──

    #[test]
    fn no_write_paths_in_production_code() {
        let src = include_str!("mod.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        for forbidden in ["fs::write", "create_dir_all", "File::create", "remove_file"] {
            assert!(
                !prod.contains(forbidden),
                "프로덕션 경로에 쓰기가 남아 있다: {forbidden}"
            );
        }
    }
}
