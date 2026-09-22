//! Obsidian vault 커넥터 — 로컬 폴더를 읽는다. 인증이 없어 파이프라인 전체를
//! 가장 싸게 end-to-end 검증할 수 있다 (설계 0020 DR-8).
//!
//! **읽기 전용이다.** vault에 쓰거나 지우는 코드를 여기 두지 않는다.

use std::path::{Path, PathBuf};

use crate::knowledge::graph::Document;

pub const SOURCE_ID: &str = "obsidian";

#[derive(Debug, Clone)]
pub struct VaultConfig {
    pub root: PathBuf,
    /// 사용자 제외 글롭. `Templates/**`, `*.excalidraw.md` 두 형태만 지원한다 —
    /// 실사용에 충분하고, 완전한 글롭 엔진은 의존성을 늘린다.
    pub exclude: Vec<String>,
    /// **색인은 하되 임베딩만 건너뛸** 경로. 어휘(FTS) 검색에는 계속 걸리고
    /// 의미 검색에서만 빠진다.
    ///
    /// 실측 vault는 청크의 98%가 `Claude Code/` 세션 로그였고, 임베딩에 ~6시간이 든다.
    /// 그렇다고 아예 빼면 "그때 그 세션에서 뭐 했더라"를 못 찾는다. 색인은 싸고
    /// 임베딩만 비싸므로, 비용이 드는 쪽만 고르는 것이 옳다.
    pub embed_exclude: Vec<String>,
}

impl VaultConfig {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            exclude: Vec::new(),
            embed_exclude: Vec::new(),
        }
    }
}

/// vault를 훑어 문서 목록을 만든다. 본문은 읽되 청킹·임베딩은 하지 않는다.
pub fn scan_vault(cfg: &VaultConfig) -> anyhow::Result<Vec<Document>> {
    let mut out = Vec::new();
    walk(&cfg.root, &cfg.root, cfg, &mut out);
    // 파일시스템 순회 순서는 플랫폼마다 다르다. 정렬해 두면 동기화 로그와 테스트가 재현된다.
    out.sort_by(|a, b| a.external_id.cmp(&b.external_id));
    Ok(out)
}

fn walk(dir: &Path, root: &Path, cfg: &VaultConfig, out: &mut Vec<Document>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return; // 권한 없는 디렉터리는 조용히 건너뛴다 — vault 전체를 실패시키지 않는다
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // 심볼릭 링크는 따라가지 않는다. `~/Documents`처럼 넓은 경로를 vault로 고르면
        // 순환 링크를 밟아 무한히 도는 일이 실제로 생긴다.
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if is_excluded_dir(&path, root, cfg) {
                continue;
            }
            walk(&path, root, cfg, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            let Some(id) = to_external_id(&path, root) else {
                continue;
            };
            if is_excluded(&id, &cfg.exclude) {
                continue;
            }
            let embed = !is_excluded(&id, &cfg.embed_exclude);
            if let Some(doc) = read_document(&path, id, embed) {
                out.push(doc);
            }
        }
    }
}

fn read_document(path: &Path, external_id: String, embed: bool) -> Option<Document> {
    let body = std::fs::read_to_string(path).ok()?;
    let updated_at = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| external_id.clone());
    Some(Document {
        source: SOURCE_ID.to_string(),
        external_id,
        kind: "document".to_string(),
        title,
        url: Some(format!("file://{}", path.display())),
        body,
        updated_at,
        embed,
    })
}

/// vault 루트 기준 상대 경로를 **항상 `/` 구분자로** 만든다.
///
/// `external_id`는 UNIQUE 키다. Windows에서 `\`로 저장되면 같은 노트가 OS마다 다른 문서가
/// 되어 재동기화 때 vault 전체가 신규로 잡힌다. Windows 실검증이 안 된 상태라
/// (설계 0020 §13) 이 정규화는 테스트로만 지켜진다.
pub fn to_external_id(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    Some(parts.join("/"))
}

/// Obsidian 내부 폴더는 항상 제외한다. 설정·플러그인·휴지통은 지식이 아니다.
fn is_excluded_dir(path: &Path, root: &Path, cfg: &VaultConfig) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string());
    if matches!(name.as_deref(), Some(".obsidian" | ".trash" | ".git")) {
        return true;
    }
    to_external_id(path, root).is_some_and(|id| is_excluded(&format!("{id}/"), &cfg.exclude))
}

/// `Templates/**` (접두사) 와 `*.excalidraw.md` (접미) 두 형태만 본다.
pub fn is_excluded(external_id: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        if let Some(prefix) = pattern.strip_suffix("**") {
            external_id.starts_with(prefix)
        } else if let Some(suffix) = pattern.strip_prefix('*') {
            external_id.ends_with(suffix)
        } else {
            external_id == pattern
        }
    })
}

/// `Source` 구현 — 전량 스캔형이다. Obsidian은 변경 알림 API가 없으므로 매번 훑고,
/// 실제 재색인 여부는 `content_hash`가 가른다.
pub struct ObsidianSource {
    pub config: VaultConfig,
}

impl ObsidianSource {
    pub fn new(config: VaultConfig) -> Self {
        Self { config }
    }
}

impl crate::knowledge::sync::Source for ObsidianSource {
    fn id(&self) -> &str {
        SOURCE_ID
    }

    /// 파일시스템 읽기라 실제로는 즉시 끝난다. 계약이 async인 것은 Gmail·Notion
    /// 때문이고(플랜 0028 DR-A), 여기서는 완성된 future를 돌려줄 뿐이다.
    fn changes<'a>(&'a self, _cursor: Option<&'a str>) -> crate::knowledge::sync::BoxChanges<'a> {
        Box::pin(async move {
            Ok(crate::knowledge::sync::SourceChanges {
                upserts: scan_vault(&self.config)?,
                deletions: Vec::new(),
                // 전량 스캔이라 커서가 없다. "언제 훑었는지"는 `last_sync`가 기록한다.
                next_cursor: None,
                full_scan: true,
                // 한 번에 vault 전량을 준다 — 이어서 부를 것이 없다.
                has_more: false,
            })
        })
    }
}
