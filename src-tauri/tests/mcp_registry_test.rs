//! MCP 게이트웨이 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::mcp_registry;

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn setup() -> (sqlx::SqlitePool, String) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir()
        .join(format!(
            "praxis-gw-test-{}-{}.sqlite",
            std::process::id(),
            n
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&path).await.unwrap();
    mcp_registry::migrate(&pool).await.unwrap();
    (pool, path)
}

#[tokio::test]
async fn add_list_toggle_remove() {
    let (pool, path) = setup().await;
    mcp_registry::add_server(
        &pool,
        "github",
        "npx",
        r#"["-y","@modelcontextprotocol/server-github"]"#,
        1,
    )
    .await
    .unwrap();
    let list = mcp_registry::list_servers(&pool).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "github");
    assert_eq!(list[0].enabled, 1);

    // ON CONFLICT 교체 (같은 name)
    mcp_registry::add_server(&pool, "github", "docker", r#"["run","x"]"#, 2)
        .await
        .unwrap();
    let list = mcp_registry::list_servers(&pool).await.unwrap();
    assert_eq!(list.len(), 1, "name 중복은 교체");
    assert_eq!(list[0].command, "docker");

    mcp_registry::set_enabled(&pool, list[0].id, false)
        .await
        .unwrap();
    assert_eq!(
        mcp_registry::list_servers(&pool).await.unwrap()[0].enabled,
        0
    );

    mcp_registry::remove_server(&pool, list[0].id)
        .await
        .unwrap();
    assert_eq!(mcp_registry::list_servers(&pool).await.unwrap().len(), 0);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn invalid_args_rejected() {
    let (pool, path) = setup().await;
    assert!(mcp_registry::add_server(&pool, "bad", "x", "not-json", 1)
        .await
        .is_err());
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn write_mcp_config_only_enabled() {
    let (pool, path) = setup().await;
    mcp_registry::add_server(&pool, "on", "npx", r#"["a"]"#, 1)
        .await
        .unwrap();
    let off_id = mcp_registry::add_server(&pool, "off", "npx", r#"["b"]"#, 1)
        .await
        .unwrap();
    mcp_registry::set_enabled(&pool, off_id, false)
        .await
        .unwrap();

    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let wt = temp_root::dir().join(format!("praxis-gw-wt-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&wt).unwrap();

    let servers = mcp_registry::list_servers(&pool).await.unwrap();
    let written = mcp_registry::write_mcp_config(&servers, &wt).unwrap();
    assert_eq!(
        written,
        mcp_registry::McpConfigWrite::Created(1),
        "활성 서버만"
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(wt.join(".mcp.json")).unwrap()).unwrap();
    assert!(json["mcpServers"]["on"].is_object(), "enabled 서버 포함");
    assert!(json["mcpServers"]["off"].is_null(), "disabled 서버 제외");
    assert_eq!(json["mcpServers"]["on"]["command"], "npx");
    assert_eq!(json["mcpServers"]["on"]["args"][0], "a");

    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn write_mcp_config_preserves_existing_user_file() {
    let (pool, path) = setup().await;
    mcp_registry::add_server(&pool, "praxis", "npx", r#"["server"]"#, 1)
        .await
        .unwrap();
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let wt = temp_root::dir().join(format!("praxis-gw-existing-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&wt).unwrap();
    let config_path = wt.join(".mcp.json");
    let original = r#"{"mcpServers":{"user":{"command":"user-mcp","args":[]}}}"#;
    std::fs::write(&config_path, original).unwrap();

    let servers = mcp_registry::list_servers(&pool).await.unwrap();
    let result = mcp_registry::write_mcp_config(&servers, &wt).unwrap();

    assert_eq!(result, mcp_registry::McpConfigWrite::SkippedExisting);
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), original);
    let _ = std::fs::remove_dir_all(&wt);
    let _ = std::fs::remove_file(&path);
}
