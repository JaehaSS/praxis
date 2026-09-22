//! Deterministic, conflict-preserving Markdown export of the active code graph.

mod model;
mod render;
mod storage;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, OnceLock};

use model::{
    CodeWikiModule, CodeWikiPageState, CodeWikiStatus, EdgeRow, NodeRow, MAX_FILES,
    MAX_GRAPH_BYTES, MAX_GRAPH_EDGE_ROWS, MAX_GRAPH_NODE_ROWS, MAX_INDEX_BYTES, MAX_OUTPUT_BYTES,
    MAX_PAGE_BYTES,
};
use sqlx::SqlitePool;

pub use model::{
    CodeWikiModule as Module, CodeWikiPageState as PageState, CodeWikiStatus as Status,
};

pub(super) static TEMP: AtomicU64 = AtomicU64::new(0);

pub async fn status(pool: &SqlitePool, root: &Path) -> anyhow::Result<CodeWikiStatus> {
    let manifest = super::manifest::scan(root)?;
    validate_source_paths(&manifest)?;
    let mut graph = super::status::load(pool, root).await?;
    if graph.active_state == "ready" {
        if let Some(run_id) = graph.active_run_id {
            if ensure_run_fingerprint(pool, run_id, &manifest.fingerprint)
                .await
                .is_err()
            {
                graph.active_state = "stale".to_owned();
            }
        }
    }
    let mut modules = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let page = storage::page_path(&file.rel_path);
        modules.push(CodeWikiModule {
            source_path: file.rel_path.clone(),
            state: page_state(
                root,
                &page,
                "module",
                Some(file),
                &manifest.fingerprint,
                &graph.active_state,
                graph.active_run_id,
            )?,
            page_path: page,
        });
    }
    let source_set: HashSet<_> = manifest
        .files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect();
    for (source_path, page) in orphan_pages(root, &source_set)? {
        modules.push(CodeWikiModule {
            source_path,
            page_path: format!("{}/{}", storage::ROOT, page),
            state: CodeWikiPageState::Orphaned,
        });
    }
    let index_path = format!("{}/index.md", storage::ROOT);
    let index_state = page_state(
        root,
        &index_path,
        "index",
        None,
        &manifest.fingerprint,
        &graph.active_state,
        graph.active_run_id,
    )?;
    Ok(CodeWikiStatus {
        graph_state: graph.active_state,
        index_path,
        index_state,
        modules,
        detail: graph.detail,
    })
}

pub async fn generate(
    pool: &SqlitePool,
    root: &Path,
    selected: Option<&str>,
    now: i64,
) -> anyhow::Result<CodeWikiStatus> {
    let _guard = GenerationGuard::acquire(root)?;
    let graph = super::status::load(pool, root).await?;
    if graph.active_state != "ready" {
        anyhow::bail!(
            "code Wiki requires a ready code graph; current state is {}",
            graph.active_state
        );
    }
    let run_id = graph
        .active_run_id
        .ok_or_else(|| anyhow::anyhow!("code Wiki active graph is unavailable"))?;
    let manifest = super::manifest::scan(root)?;
    validate_source_paths(&manifest)?;
    ensure_run_fingerprint(pool, run_id, &manifest.fingerprint).await?;
    if manifest.files.len() > MAX_FILES {
        anyhow::bail!("code Wiki refuses to generate more than {MAX_FILES} source files");
    }
    let selected_files = select_files(&manifest, selected)?;
    let _disk_lock = storage::lock(root)?;
    let (nodes, edges) = graph_rows(pool, run_id).await?;
    let missing_edges = edge_states(pool, run_id).await?;
    let node_groups = group_nodes(nodes);
    let edge_groups = group_edges(edges);
    let source_set: HashSet<_> = manifest
        .files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect();
    let orphans = orphan_pages(root, &source_set)?;
    let mut output = Vec::new();
    let mut output_bytes = 0;
    for file in &selected_files {
        let file_nodes = node_groups.get(&file.rel_path).cloned().unwrap_or_default();
        let file_edges = edge_groups.get(&file.rel_path).cloned().unwrap_or_default();
        let content = render::page(
            &file.rel_path,
            &file.content_hash,
            &manifest.fingerprint,
            run_id,
            now,
            &file_nodes,
            &file_edges,
            missing_edges.get(&file.rel_path).map(String::as_str),
        );
        size_check(&content, MAX_PAGE_BYTES, &file.rel_path)?;
        push_output(
            &mut output,
            &mut output_bytes,
            storage::page_path(&file.rel_path),
            "module",
            content,
        )?;
    }
    let selected_paths: HashSet<_> = selected_files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect();
    let all_modules: Vec<_> = manifest
        .files
        .iter()
        .map(|file| -> anyhow::Result<_> {
            let page = storage::page_path(&file.rel_path);
            let state = if selected_paths.contains(file.rel_path.as_str()) {
                CodeWikiPageState::Ready
            } else {
                page_state(
                    root,
                    &page,
                    "module",
                    Some(file),
                    &manifest.fingerprint,
                    "ready",
                    Some(run_id),
                )?
            };
            Ok((
                file.rel_path.clone(),
                format!("modules/{}.md", file.rel_path),
                state,
            ))
        })
        .collect::<anyhow::Result<_>>()?;
    let index = render::index(&manifest.fingerprint, run_id, now, &all_modules, &orphans);
    size_check(&index, MAX_INDEX_BYTES, "index")?;
    push_output(
        &mut output,
        &mut output_bytes,
        format!("{}/index.md", storage::ROOT),
        "index",
        index,
    )?;
    let expected: Vec<_> = output
        .iter()
        .map(|(path, kind, _)| (path.as_str(), *kind))
        .collect();
    let preimages = storage::preflight(root, &expected)?;
    for ((path, kind, _), preimage) in output.iter().zip(&preimages) {
        if *kind == "module" {
            verify_existing_module(&preimage.bytes, path)?;
        }
    }
    ensure_fresh(pool, root, run_id, &manifest.fingerprint).await?;
    for ((_, kind, content), preimage) in output.iter().zip(&preimages) {
        if *kind == "module" {
            storage::write(root, preimage, content)?;
        }
    }
    ensure_fresh(pool, root, run_id, &manifest.fingerprint).await?;
    let ((_, _, index), preimage) = output.last().zip(preimages.last()).expect("index output");
    storage::write(root, preimage, index)?;
    ensure_fresh(pool, root, run_id, &manifest.fingerprint).await?;
    status(pool, root).await
}

fn select_files<'a>(
    manifest: &'a super::manifest::SourceManifest,
    selected: Option<&str>,
) -> anyhow::Result<Vec<&'a super::manifest::ManifestFile>> {
    let Some(selected) = selected else {
        return Ok(manifest.files.iter().collect());
    };
    let file = manifest
        .files
        .iter()
        .find(|file| file.rel_path == selected)
        .ok_or_else(|| {
            anyhow::anyhow!("selected code Wiki path is not a current indexed source: {selected}")
        })?;
    Ok(vec![file])
}

fn page_state(
    root: &Path,
    path: &str,
    kind: &str,
    file: Option<&super::manifest::ManifestFile>,
    fingerprint: &str,
    graph_state: &str,
    run_id: Option<i64>,
) -> anyhow::Result<CodeWikiPageState> {
    let Some(text) = storage::read(root, path)? else {
        return Ok(CodeWikiPageState::Missing);
    };
    let Some((meta, _)) = render::parse_document(&text) else {
        return Ok(CodeWikiPageState::Conflict);
    };
    if meta.kind != kind {
        return Ok(CodeWikiPageState::Conflict);
    }
    if let Some(file) = file {
        if meta.source_path.as_deref() != Some(&file.rel_path)
            || meta.source_hash.as_deref() != Some(&file.content_hash)
        {
            return Ok(CodeWikiPageState::Stale);
        }
    }
    // 소스가 그대로여도 그래프 세대가 바뀌면 페이지의 참조 절은 낡았다. 서버를 설치하고 다시
    // 빌드하면 `source_hash`·`source_fingerprint`는 둘 다 같으므로, run을 함께 보지 않으면
    // "참조 분석 없음"이라는 거짓이 `Ready`인 채 남는다(설계 0065 DR-6·R8).
    if meta.source_fingerprint != fingerprint
        || graph_state != "ready"
        || run_id.is_some_and(|active| active != meta.run_id)
    {
        return Ok(CodeWikiPageState::Stale);
    }
    Ok(CodeWikiPageState::Ready)
}

fn verify_existing_module(bytes: &Option<Vec<u8>>, path: &str) -> anyhow::Result<()> {
    let Some(bytes) = bytes else {
        return Ok(());
    };
    let text = std::str::from_utf8(bytes)?;
    let Some((meta, _)) = render::parse_document(text) else {
        anyhow::bail!("refusing to overwrite modified code Wiki page: {path}");
    };
    let expected = path
        .strip_prefix(&format!("{}/modules/", storage::ROOT))
        .and_then(|value| value.strip_suffix(".md"));
    if meta.source_path.as_deref() != expected {
        anyhow::bail!("refusing to overwrite a code Wiki page at a different source path: {path}");
    }
    Ok(())
}

/// 엣지를 만들지 못한 파일과 그 사유. 페이지 본문에 적어 "참조 없음"으로 읽히는 것을 막는다
/// (설계 0065 DR-6). 행 수는 이 run의 파일 수를 넘지 않고, 그것은 `MAX_FILES`로 이미 막혀 있다.
async fn edge_states(pool: &SqlitePool, run_id: i64) -> anyhow::Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT rel_path, edge_state FROM code_graph_files \
         WHERE run_id=? AND edge_state IS NOT NULL",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

async fn graph_rows(
    pool: &SqlitePool,
    run_id: i64,
) -> anyhow::Result<(Vec<NodeRow>, Vec<EdgeRow>)> {
    let mut tx = pool.begin().await?;
    let nodes: (i64, i64) = sqlx::query_as("SELECT count(*), coalesce(sum(length(CAST(f.rel_path AS BLOB)) + length(CAST(n.name AS BLOB)) + coalesce(length(CAST(n.container AS BLOB)), 0)), 0) FROM code_graph_nodes n JOIN code_graph_files f ON f.id=n.file_id WHERE n.run_id=?").bind(run_id).fetch_one(&mut *tx).await?;
    let edges: (i64, i64) = sqlx::query_as("SELECT count(*), coalesce(sum(length(CAST(sf.rel_path AS BLOB)) + length(CAST(sn.name AS BLOB)) + length(CAST(df.rel_path AS BLOB)) + length(CAST(dn.name AS BLOB))), 0) FROM code_graph_edges e JOIN code_graph_nodes sn ON sn.id=e.src_id JOIN code_graph_files sf ON sf.id=sn.file_id JOIN code_graph_nodes dn ON dn.id=e.dst_id JOIN code_graph_files df ON df.id=dn.file_id WHERE e.run_id=? AND e.rel='references'").bind(run_id).fetch_one(&mut *tx).await?;
    validate_graph_bounds(nodes, edges)?;
    let nodes = sqlx::query_as("SELECT f.rel_path, n.name, n.kind, n.container, n.sel_start_line, n.sel_start_char FROM code_graph_nodes n JOIN code_graph_files f ON f.id=n.file_id WHERE n.run_id=? ORDER BY f.rel_path, n.sel_start_line, n.sel_start_char, n.name LIMIT ?").bind(run_id).bind((MAX_GRAPH_NODE_ROWS + 1) as i64).fetch_all(&mut *tx).await?;
    let edges = sqlx::query_as("SELECT sf.rel_path AS src_path, sn.name AS src_name, sn.sel_start_line AS src_line, df.rel_path AS dst_path, dn.name AS dst_name, dn.sel_start_line AS dst_line FROM code_graph_edges e JOIN code_graph_nodes sn ON sn.id=e.src_id JOIN code_graph_files sf ON sf.id=sn.file_id JOIN code_graph_nodes dn ON dn.id=e.dst_id JOIN code_graph_files df ON df.id=dn.file_id WHERE e.run_id=? AND e.rel='references' ORDER BY sf.rel_path, sn.sel_start_line, df.rel_path, dn.sel_start_line LIMIT ?").bind(run_id).bind((MAX_GRAPH_EDGE_ROWS + 1) as i64).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok((nodes, edges))
}

fn validate_graph_bounds(nodes: (i64, i64), edges: (i64, i64)) -> anyhow::Result<()> {
    let rows = (nodes.0 as usize, edges.0 as usize);
    let bytes = nodes.1.saturating_add(edges.1) as usize;
    if rows.0 > MAX_GRAPH_NODE_ROWS || rows.1 > MAX_GRAPH_EDGE_ROWS || bytes > MAX_GRAPH_BYTES {
        anyhow::bail!("code Wiki graph data exceeds bounded generation limits");
    }
    Ok(())
}

fn push_output(
    output: &mut Vec<(String, &'static str, String)>,
    total: &mut usize,
    path: String,
    kind: &'static str,
    content: String,
) -> anyhow::Result<()> {
    *total = total
        .checked_add(content.len())
        .ok_or_else(|| anyhow::anyhow!("code Wiki output size overflow"))?;
    if *total > MAX_OUTPUT_BYTES {
        anyhow::bail!(
            "code Wiki rendered output exceeds the {MAX_OUTPUT_BYTES} byte aggregate limit"
        );
    }
    output.push((path, kind, content));
    Ok(())
}

fn group_nodes(nodes: Vec<NodeRow>) -> HashMap<String, Vec<NodeRow>> {
    let mut groups = HashMap::new();
    for node in nodes {
        groups
            .entry(node.rel_path.clone())
            .or_insert_with(Vec::new)
            .push(node);
    }
    groups
}

fn group_edges(edges: Vec<EdgeRow>) -> HashMap<String, Vec<EdgeRow>> {
    let mut groups: HashMap<String, Vec<EdgeRow>> = HashMap::new();
    for edge in edges {
        groups
            .entry(edge.src_path.clone())
            .or_default()
            .push(edge.clone());
        if edge.src_path != edge.dst_path {
            groups.entry(edge.dst_path.clone()).or_default().push(edge);
        }
    }
    groups
}

fn validate_source_paths(manifest: &super::manifest::SourceManifest) -> anyhow::Result<()> {
    for file in &manifest.files {
        if file.rel_path.chars().any(char::is_control) || file.rel_path.contains('\u{FFFD}') {
            anyhow::bail!("code Wiki refuses a source path with control or non-Unicode characters");
        }
    }
    Ok(())
}

fn orphan_pages(root: &Path, current: &HashSet<&str>) -> anyhow::Result<Vec<(String, String)>> {
    let mut found = Vec::new();
    visit_modules(root, &root.join(storage::ROOT).join("modules"), &mut found)?;
    found.retain(|(source, _)| !current.contains(source.as_str()));
    found.sort();
    Ok(found)
}

fn visit_modules(
    root: &Path,
    directory: &Path,
    found: &mut Vec<(String, String)>,
) -> anyhow::Result<()> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            anyhow::bail!("code Wiki modules contains a symbolic link");
        }
        if kind.is_dir() {
            visit_modules(root, &entry.path(), found)?;
            continue;
        }
        if !kind.is_file() || entry.path().extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(text) = storage::read(root, &relative)? {
            if let Some((meta, _)) = render::parse_document(&text) {
                if meta.kind == "module" {
                    if let Some(source) = meta.source_path {
                        found.push((
                            source,
                            relative
                                .trim_start_matches(&format!("{}/", storage::ROOT))
                                .to_owned(),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn size_check(content: &str, limit: usize, name: &str) -> anyhow::Result<()> {
    if content.len() > limit {
        anyhow::bail!("code Wiki {name} exceeds the {} byte limit", limit);
    }
    Ok(())
}

async fn ensure_fresh(
    pool: &SqlitePool,
    root: &Path,
    run_id: i64,
    fingerprint: &str,
) -> anyhow::Result<()> {
    let active = super::generation::active_run_id(pool, &root.to_string_lossy()).await?;
    if active != Some(run_id)
        || super::manifest::scan(root)?.fingerprint != fingerprint
        || ensure_run_fingerprint(pool, run_id, fingerprint)
            .await
            .is_err()
    {
        anyhow::bail!("code Wiki source or graph changed during generation; some pages may have been updated, rerun generation");
    }
    Ok(())
}

async fn ensure_run_fingerprint(
    pool: &SqlitePool,
    run_id: i64,
    fingerprint: &str,
) -> anyhow::Result<()> {
    let (stored,): (String,) =
        sqlx::query_as("SELECT source_fingerprint FROM code_graph_runs WHERE id=?")
            .bind(run_id)
            .fetch_one(pool)
            .await?;
    if stored != fingerprint {
        anyhow::bail!(
            "code Wiki active graph fingerprint does not match the captured source manifest"
        );
    }
    Ok(())
}

struct GenerationGuard {
    root: PathBuf,
}
impl GenerationGuard {
    fn acquire(root: &Path) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        let set = generation_set();
        let mut set = set
            .lock()
            .map_err(|_| anyhow::anyhow!("code Wiki generation lock is poisoned"))?;
        if !set.insert(root.clone()) {
            anyhow::bail!("code Wiki generation is already running for this worktree");
        }
        Ok(Self { root })
    }
}
impl Drop for GenerationGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = generation_set().lock() {
            set.remove(&self.root);
        }
    }
}
fn generation_set() -> &'static Mutex<HashSet<PathBuf>> {
    static SET: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

#[cfg(test)]
mod tests;
