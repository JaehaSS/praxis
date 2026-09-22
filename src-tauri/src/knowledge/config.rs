//! 소스 설정 저장 — `knowledge_sources.config`의 JSON.
//!
//! **자격 증명은 여기 두지 않는다** (설계 0020 DR-5). DB 파일은 백업·동기화 폴더로
//! 복사되기 쉬워서, 토큰이 있으면 파일 한 번 유출이 계정 탈취가 된다.
//! Obsidian은 인증이 없어 경로만 담는다.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::source::gmail::SOURCE_ID as GMAIL_SOURCE_ID;
use super::source::obsidian::{ObsidianSource, VaultConfig, SOURCE_ID};
use super::sync::{BoxChanges, Source, SourceChanges};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObsidianConfig {
    /// **목록**이다. 실측에서 `.obsidian`이 두 곳에 있었고(하나는 과거 흔적),
    /// vault 경로는 남고 이동하고 중첩된다 (DR-11).
    #[serde(default)]
    pub vaults: Vec<VaultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultEntry {
    pub root: String,
    #[serde(default)]
    pub exclude: Vec<String>,
    /// 색인은 하되 임베딩만 건너뛸 경로 (`VaultConfig::embed_exclude` 참조).
    #[serde(default)]
    pub embed_exclude: Vec<String>,
}

/// Gmail 소스 설정.
///
/// **여기 없는 것**: client_secret, refresh token. 둘 다 키체인이다 (DR-5,
/// `source::gmail::{CLIENT_SECRET_KEY, REFRESH_TOKEN_KEY}`).
/// `client_id`는 공개값이라 남는다 — 없으면 설정을 복원할 수 없다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailConfig {
    /// 흡수 범위 (DR-12). 라벨이 아니라 **카테고리 제외 + 시간창**인 이유는
    /// 사용자가 라벨을 안 쓰면 라벨 기준이 무용지물이기 때문이다. 프로모션·소셜은
    /// 지식 가치가 0에 가까운데 볼륨은 흔히 절반을 넘어, 백필 시간의 최대 절감원이다.
    #[serde(default = "default_query")]
    pub query: String,
    #[serde(default)]
    pub client_id: String,
}

/// 좁게 시작해 넓히면 증분 백필이 부족분만 채운다. 넓게 시작해 좁히면 이미 25분과
/// 수백 MB를 쓴 뒤다 — **되돌리기 싼 쪽으로** 기본값을 잡는다 (DR-12).
fn default_query() -> String {
    "-category:promotions -category:social -category:forums newer_than:3y".to_string()
}

impl Default for GmailConfig {
    fn default() -> Self {
        Self {
            query: default_query(),
            client_id: String::new(),
        }
    }
}

pub async fn load_gmail(pool: &SqlitePool) -> anyhow::Result<GmailConfig> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT config FROM knowledge_sources WHERE id = ?")
            .bind(GMAIL_SOURCE_ID)
            .fetch_optional(pool)
            .await?;
    Ok(row
        .and_then(|r| r.0)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// 저장은 `status`를 건드리지 않는다. 설정을 적었다는 것과 인증이 살아 있다는 것은
/// 다른 사실이고, Gmail은 후자가 OAuth 결과로만 정해진다 (Obsidian과 갈리는 지점).
pub async fn save_gmail(pool: &SqlitePool, cfg: &GmailConfig) -> anyhow::Result<()> {
    let json = serde_json::to_string(cfg)?;
    sqlx::query(
        "INSERT INTO knowledge_sources (id, status, config) VALUES (?, 'disconnected', ?) \
         ON CONFLICT(id) DO UPDATE SET config = excluded.config",
    )
    .bind(GMAIL_SOURCE_ID)
    .bind(json)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_obsidian(pool: &SqlitePool) -> anyhow::Result<ObsidianConfig> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT config FROM knowledge_sources WHERE id = ?")
            .bind(SOURCE_ID)
            .fetch_optional(pool)
            .await?;
    Ok(row
        .and_then(|r| r.0)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

pub async fn save_obsidian(pool: &SqlitePool, cfg: &ObsidianConfig) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    save_obsidian_with(&mut tx, cfg).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn save_obsidian_with(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    cfg: &ObsidianConfig,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(cfg)?;
    sqlx::query(
        "INSERT INTO knowledge_sources (id, status, config) VALUES (?, 'connected', ?) \
         ON CONFLICT(id) DO UPDATE SET config = excluded.config, status = 'connected'",
    )
    .bind(SOURCE_ID)
    .bind(json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 여러 vault를 한 소스로 합친다.
///
/// vault마다 상대 경로가 겹칠 수 있으므로(`note.md`가 양쪽에) `external_id` 앞에
/// **vault 라벨**(루트 디렉터리명)을 붙인다. 인덱스를 쓰면 목록 순서를 바꿀 때
/// 전체가 신규 문서로 잡힌다.
pub struct MultiVault {
    pub entries: Vec<VaultEntry>,
}

impl Source for MultiVault {
    fn id(&self) -> &str {
        SOURCE_ID
    }

    fn changes<'a>(&'a self, _cursor: Option<&'a str>) -> BoxChanges<'a> {
        Box::pin(async move {
            // 라벨이 겹치면 서로 다른 vault의 같은 상대경로가 하나의 `external_id`가 되어
            // UNIQUE(source, external_id) 위에서 **서로를 덮어쓴다**. 노트가 조용히 사라지는
            // 형태라 알아채기 어렵다. 인덱스를 붙여 회피할 수도 있지만 목록 순서를 바꾸면
            // 전체가 신규 문서가 되므로, 사람이 폴더 이름을 정리하게 하는 편이 낫다.
            let mut seen = std::collections::HashSet::new();
            for entry in &self.entries {
                let label = vault_label(&entry.root);
                if !seen.insert(label.clone()) {
                    anyhow::bail!(
                        "vault 폴더 이름이 겹칩니다: '{label}'. 서로 다른 이름의 폴더를 선택하세요."
                    );
                }
            }

            let mut upserts = Vec::new();
            for entry in &self.entries {
                let label = vault_label(&entry.root);
                let source = ObsidianSource::new(VaultConfig {
                    root: entry.root.clone().into(),
                    exclude: entry.exclude.clone(),
                    embed_exclude: entry.embed_exclude.clone(),
                });
                for mut doc in source.changes(None).await?.upserts {
                    doc.external_id = format!("{label}/{}", doc.external_id);
                    upserts.push(doc);
                }
            }
            upserts.sort_by(|a, b| a.external_id.cmp(&b.external_id));
            Ok(SourceChanges {
                upserts,
                deletions: Vec::new(),
                next_cursor: None,
                full_scan: true,
                has_more: false,
            })
        })
    }
}

fn vault_label(root: &str) -> String {
    std::path::Path::new(root)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "vault".to_string())
}
