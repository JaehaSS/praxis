//! MCP 레지스트리 (Phase 7) — MCP 서버 레지스트리 + per-task `.mcp.json` 주입.
//!
//! 설계: PTY 출력 가로채기 대신 **Claude Code의 네이티브 MCP**를 활용한다.
//! 등록된 MCP 서버 설정을 작업 worktree에 `.mcp.json`으로 써주면 에이전트가 직접 로드한다.
//! **시크릿은 저장하지 않는다** — MCP 서버 프로세스는 spawn된 에이전트의 상속 환경변수를 사용.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct McpServer {
    pub id: i64,
    pub name: String,
    pub command: String,
    pub args: String, // JSON 배열 문자열
    pub enabled: i64, // 0/1
    pub created_at: i64,
}

/// 작업 경계에 MCP 설정을 주입한 결과. 기존 파일은 사용자 소유로 보고 절대 덮어쓰지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpConfigWrite {
    NoServers,
    Created(usize),
    SkippedExisting,
}

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS mcp_servers (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  name       TEXT NOT NULL UNIQUE,
  command    TEXT NOT NULL,
  args       TEXT NOT NULL DEFAULT '[]',
  enabled    INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);
"#;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(MIGRATION).execute(pool).await?;
    Ok(())
}

/// MCP 서버 등록 (name 중복 시 교체). args는 JSON 배열 문자열.
pub async fn add_server(
    pool: &SqlitePool,
    name: &str,
    command: &str,
    args_json: &str,
    now: i64,
) -> anyhow::Result<i64> {
    // args 유효성 검증 (JSON 배열)
    let _: Vec<String> = serde_json::from_str(args_json)
        .map_err(|_| anyhow::anyhow!("args는 JSON 문자열 배열이어야 함"))?;
    let id = sqlx::query(
        "INSERT INTO mcp_servers (name, command, args, enabled, created_at) VALUES (?, ?, ?, 1, ?) \
         ON CONFLICT(name) DO UPDATE SET command = excluded.command, args = excluded.args",
    )
    .bind(name)
    .bind(command)
    .bind(args_json)
    .bind(now)
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(id)
}

pub async fn list_servers(pool: &SqlitePool) -> anyhow::Result<Vec<McpServer>> {
    Ok(
        sqlx::query_as::<_, McpServer>("SELECT * FROM mcp_servers ORDER BY name")
            .fetch_all(pool)
            .await?,
    )
}

pub async fn remove_server(pool: &SqlitePool, id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_enabled(pool: &SqlitePool, id: i64, enabled: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 활성 MCP 서버가 있고 사용자 파일이 없을 때만 worktree에 `.mcp.json`을 새로 생성한다.
pub fn write_mcp_config(
    servers: &[McpServer],
    worktree_path: &Path,
) -> anyhow::Result<McpConfigWrite> {
    let mut obj = serde_json::Map::new();
    for s in servers.iter().filter(|s| s.enabled != 0) {
        let args: Vec<String> = serde_json::from_str(&s.args).unwrap_or_default();
        obj.insert(
            s.name.clone(),
            json!({ "command": s.command, "args": args }),
        );
    }
    if obj.is_empty() {
        return Ok(McpConfigWrite::NoServers);
    }
    let n = obj.len();
    let config = json!({ "mcpServers": obj });
    let path = worktree_path.join(".mcp.json");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => return Ok(McpConfigWrite::SkippedExisting),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(McpConfigWrite::SkippedExisting)
        }
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = file.write_all(serde_json::to_string_pretty(&config)?.as_bytes()) {
        let _ = std::fs::remove_file(&path);
        return Err(error.into());
    }
    Ok(McpConfigWrite::Created(n))
}
