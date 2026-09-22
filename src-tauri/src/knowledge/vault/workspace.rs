//! File-first Wiki workspace. The installed harness owns Markdown link semantics.
use super::files::{hash, open_scoped_verified, vault_root, VaultRoot};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    collections::HashSet,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

mod storage;
#[cfg(test)]
mod tests;

const MAX_PAGE: u64 = 2 * 1024 * 1024;
const MAX_GRAPH: u64 = 24 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
pub struct Page {
    pub id: String,
    pub path: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    /// Harness front-matter `type`, defaulting to `page`. `kind` here because `type` is a Rust keyword.
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub scope: String,
    pub body: String,
    pub source_prefix: String,
    pub outgoing: Vec<String>,
    pub backlinks: Vec<String>,
    #[serde(default)]
    pub sha256: String,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct LinkEvidence {
    pub line: usize,
    pub target: String,
    pub syntax: String,
    pub anchor: String,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub evidence: Vec<LinkEvidence>,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Diagnostic {
    pub kind: String,
    pub source: String,
    #[serde(default)]
    pub target: String,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct Graph {
    pub schema_version: u32,
    pub nodes: Vec<Page>,
    pub edges: Vec<Edge>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(default)]
    pub writable: bool,
}
#[derive(Debug, Serialize)]
pub struct Document {
    pub path: String,
    pub content: String,
    pub sha256: String,
}

pub(super) fn page_path(path: &str) -> anyhow::Result<()> {
    let parts: Vec<_> = path.split('/').collect();
    if parts.is_empty()
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || parts.iter().any(|s| {
            s.is_empty()
                || s.starts_with('.')
                || ["node_modules", "__pycache__", "raw", "lint"].contains(s)
        })
        || !path.to_lowercase().ends_with(".md")
        || parts.last().is_some_and(|s| {
            ["agents.md", "claude.md", "gemini.md"].contains(&s.to_lowercase().as_str())
        })
        || path.ends_with("wiki/index.md")
        || path.ends_with("wiki/log.md")
    {
        anyhow::bail!("허용된 창고 하위 Markdown 경로를 입력하세요 (숨김·지침·생성 인덱스 제외)");
    }
    Ok(())
}

fn read_at(root: &VaultRoot, path: &str) -> anyhow::Result<Document> {
    page_path(path)?;
    let file = open_scoped_verified(&root.path, root.device, root.inode, path)?;
    if !file.metadata()?.is_file() {
        anyhow::bail!("일반 Markdown 파일만 열 수 있습니다");
    }
    let mut bytes = Vec::new();
    file.take(MAX_PAGE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PAGE {
        anyhow::bail!("문서가 2MiB를 넘습니다");
    }
    let sha256 = hash(&bytes);
    let content = String::from_utf8(bytes)?;
    if content.contains("<!-- knowledge-harness:generated -->") {
        anyhow::bail!("자동 생성 문서는 이 화면에서 수정하지 않습니다");
    }
    Ok(Document {
        path: path.into(),
        content,
        sha256,
    })
}

async fn writable(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT writable FROM vaults WHERE id = ? AND enabled = 1")
            .bind(id)
            .fetch_one(pool)
            .await?
            != 0,
    )
}

pub async fn read(pool: &SqlitePool, id: &str, path: &str) -> anyhow::Result<Document> {
    let _guard = super::shared_admission(pool).await?;
    read_at(&vault_root(pool, id).await?, path)
}

fn parser_scripts(home: &Path) -> anyhow::Result<PathBuf> {
    [".codex/skills/knowledge-harness/scripts", ".claude/skills/knowledge-harness/scripts"]
        .iter().map(|p| home.join(p)).find(|p| p.join("knowledge_graph.py").is_file())
        .ok_or_else(|| anyhow::anyhow!("knowledge-harness의 문서 그래프 스킬이 필요합니다. ~/.codex/skills 또는 ~/.claude/skills에 설치한 뒤 새로 고침하세요."))
}

async fn run_parser(root: &Path, scripts: &Path) -> anyhow::Result<Graph> {
    let python = crate::reviewer::which("python3")
        .ok_or_else(|| anyhow::anyhow!("위키 관계 탐색에는 Python 3.10 이상이 필요합니다"))?;
    let mut child = tokio::process::Command::new(python)
        .args(["-I", "-B", "-c", include_str!("workspace/graph_adapter.py")])
        .arg(scripts)
        .arg(root)
        .current_dir(scripts)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("graph stdout missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("graph stderr missing"))?;
    let read_output = async move {
        let mut bytes = Vec::new();
        stdout.take(MAX_GRAPH + 1).read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    };
    let read_errors = async move {
        let mut bytes = Vec::new();
        stderr.take(8192).read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    };
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::try_join!(read_output, read_errors, child.wait())
    })
    .await;
    let (out, err, status) = match result {
        Ok(result) => result?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            anyhow::bail!("그래프 계산이 20초를 넘었습니다. 폴더 크기를 줄이거나 다시 시도하세요.");
        }
    };
    if !status.success() {
        anyhow::bail!(
            "그래프를 읽지 못했습니다: {}",
            String::from_utf8_lossy(&err)
        );
    }
    if out.len() as u64 > MAX_GRAPH {
        anyhow::bail!("그래프가 출력 한도를 넘습니다");
    }
    serde_json::from_slice(&out)
        .map_err(|e| anyhow::anyhow!("하네스 그래프 형식이 호환되지 않습니다: {e}"))
}

fn validate_graph(root: &VaultRoot, graph: &mut Graph) -> anyhow::Result<()> {
    if graph.schema_version != 1 || graph.nodes.len() > 1000 || graph.edges.len() > 100_000 {
        anyhow::bail!("지원하지 않는 그래프 형식 또는 크기입니다");
    }
    let mut ids = HashSet::new();
    for page in &mut graph.nodes {
        if !ids.insert(page.id.clone()) {
            anyhow::bail!("중복 문서 ID입니다");
        }
        let actual = read_at(root, &page.path)?;
        if actual
            .content
            .trim_start_matches('\u{feff}')
            .replace("\r\n", "\n")
            != format!("{}{}", page.source_prefix, page.body)
        {
            anyhow::bail!("그래프를 읽는 동안 문서가 변경됐습니다. 새로 고침하세요.");
        }
        page.sha256 = actual.sha256;
    }
    if graph
        .edges
        .iter()
        .any(|e| !ids.contains(&e.source) || !ids.contains(&e.target))
        || graph.nodes.iter().any(|n| {
            n.outgoing
                .iter()
                .chain(&n.backlinks)
                .any(|id| !ids.contains(id))
        })
    {
        anyhow::bail!("그래프가 없는 문서를 가리킵니다");
    }
    Ok(())
}

pub async fn graph(pool: &SqlitePool, id: &str) -> anyhow::Result<Graph> {
    let _guard = super::shared_admission(pool).await?;
    let root = vault_root(pool, id).await?;
    let home =
        std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("홈 폴더를 찾을 수 없습니다"))?;
    let mut result = run_parser(&root.path, &parser_scripts(Path::new(&home))?).await?;
    let verified = vault_root(pool, id).await?;
    validate_graph(&verified, &mut result)?;
    result.writable = writable(pool, id).await?;
    Ok(result)
}

pub async fn save(
    pool: &SqlitePool,
    id: &str,
    path: &str,
    content: &str,
    expected_hash: Option<&str>,
) -> anyhow::Result<Document> {
    let _guard = super::exclusive_admission(pool).await?;
    if !writable(pool, id).await? {
        anyhow::bail!("읽기 전용 창고입니다");
    }
    page_path(path)?;
    if content.len() as u64 > MAX_PAGE {
        anyhow::bail!("문서가 2MiB를 넘습니다");
    }
    if content.contains("<!-- knowledge-harness:generated -->") {
        anyhow::bail!("자동 생성 표시를 편집 문서에 넣을 수 없습니다");
    }
    let root = vault_root(pool, id).await?;
    storage::save(&root, path, content, expected_hash)?;
    read_at(&root, path)
}

pub async fn trash(
    pool: &SqlitePool,
    id: &str,
    path: &str,
    expected_hash: &str,
) -> anyhow::Result<()> {
    let _guard = super::exclusive_admission(pool).await?;
    if !writable(pool, id).await? {
        anyhow::bail!("읽기 전용 창고입니다");
    }
    let root = vault_root(pool, id).await?;
    storage::trash_with(&root, path, expected_hash, |p| {
        crate::fsapi::mutate::trash(p)
    })
}
