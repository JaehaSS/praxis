//! Disposable SQLite fixture mounts for declared workflow resources.
//!
//! This driver never mounts the operator's fixture itself.  A write claim gets a fresh private
//! copy, while a read claim gets the immutable frozen copy created by a prior write reset.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};

use crate::workflow::{lifecycle::ResourceClaimView, resources::StepLease, store::WorkflowStore};

const MAX_FIXTURE_BYTES: u64 = 1024 * 1024 * 1024;
const RECEIPT_FILE: &str = "fixture-receipt.json";
const DATABASE_FILE: &str = "database.sqlite";
static SEED_STAGING: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DbProfile {
    pub resource_id: String,
    /// A regular, offline SQLite seed selected by runner configuration.
    pub fixture_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbMount {
    /// Directory bind mounted into the container. SQLite DELETE mode needs a sibling journal.
    pub host_path: PathBuf,
    pub container_path: PathBuf,
    pub database_path: PathBuf,
    pub read_only: bool,
    pub environment_hash: String,
    pub generation: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureReceipt {
    version: u32,
    resource_id: String,
    physical_identity: String,
    fixture_hash: String,
    generation: i64,
}

/// Prepares one resource-mounted database. `private_root` is a Runner-owned workflow workspace,
/// never a project path or a user-selected database location.
pub async fn prepare(
    store: &WorkflowStore,
    lease: &StepLease,
    profile: &DbProfile,
    private_root: &Path,
) -> Result<DbMount> {
    let claim = db_claim(store, lease, &profile.resource_id).await?;
    let fixture = validate_fixture(&profile.fixture_path).await?;
    let root = private_shared_db_root(private_root)?;
    let resource_dir = root.join(hash_bytes(claim.physical_identity.as_bytes()));
    match claim.mode.as_str() {
        "exclusive_write" => {
            reset_write_fixture(&root, &resource_dir, &claim, &fixture, lease).await
        }
        "shared_read" => prepare_read_fixture(&root, &resource_dir, &claim, &fixture, lease).await,
        "capacity" => bail!("capacity resources cannot be mounted as workflow SQLite fixtures"),
        _ => bail!("unsupported workflow SQLite resource mode"),
    }
}

async fn db_claim(
    store: &WorkflowStore,
    lease: &StepLease,
    resource_id: &str,
) -> Result<ResourceClaimView> {
    if resource_id.is_empty() || resource_id.len() > 256 {
        bail!("invalid SQLite fixture resource id");
    }
    let claims = store.step_resource_claims(lease).await?;
    let claim = claims
        .into_iter()
        .find(|claim| claim.resource_id == resource_id)
        .context("workflow step has no claimed SQLite fixture resource")?;
    if claim.generation != lease.generation {
        bail!("SQLite fixture resource claim generation is stale");
    }
    Ok(claim)
}

async fn reset_write_fixture(
    root: &Path,
    resource_dir: &Path,
    claim: &ResourceClaimView,
    fixture: &Path,
    lease: &StepLease,
) -> Result<DbMount> {
    // This is the only deletion path. It is reached only after a current exclusive DB claim was
    // read from the FULL ledger, and every target component is generated below `shared-db`.
    ensure!(
        resource_dir.starts_with(root),
        "invalid private SQLite fixture target"
    );
    if let Ok(metadata) = fs::symlink_metadata(resource_dir) {
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "private SQLite fixture directory is not a regular directory"
        );
        fs::remove_dir_all(resource_dir)?;
    }
    fs::create_dir_all(resource_dir)?;
    let generation_dir = resource_dir.join(format!("generation-{}", lease.generation));
    fs::create_dir(&generation_dir)?;
    let write_dir = generation_dir.join("write");
    let frozen_dir = generation_dir.join("frozen");
    fs::create_dir(&write_dir)?;
    fs::create_dir(&frozen_dir)?;
    let write_path = write_dir.join(DATABASE_FILE);
    let frozen_path = frozen_dir.join(DATABASE_FILE);
    let fixture_hash = copy_fixture(fixture, &[&write_path, &frozen_path])?;
    validate_sqlite_delete(&write_path).await?;
    validate_sqlite_delete(&frozen_path).await?;
    let receipt = FixtureReceipt {
        version: 1,
        resource_id: claim.resource_id.clone(),
        physical_identity: claim.physical_identity.clone(),
        fixture_hash: fixture_hash.clone(),
        generation: lease.generation,
    };
    write_receipt(&resource_dir.join(RECEIPT_FILE), &receipt)?;
    sync_dir(&write_dir)?;
    sync_dir(&frozen_dir)?;
    sync_dir(&generation_dir)?;
    sync_dir(resource_dir)?;
    Ok(DbMount {
        host_path: write_dir,
        container_path: container_mount_path(claim),
        database_path: container_mount_path(claim).join(DATABASE_FILE),
        read_only: false,
        environment_hash: environment_hash(claim, &fixture_hash, lease.generation, false),
        generation: lease.generation,
    })
}

async fn prepare_read_fixture(
    root: &Path,
    resource_dir: &Path,
    claim: &ResourceClaimView,
    fixture: &Path,
    _lease: &StepLease,
) -> Result<DbMount> {
    ensure!(
        resource_dir.starts_with(root),
        "invalid private SQLite fixture target"
    );
    let receipt = match fs::symlink_metadata(resource_dir) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "private SQLite fixture directory is not a regular directory"
            );
            read_receipt(&resource_dir.join(RECEIPT_FILE))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            publish_first_read_seed(root, resource_dir, claim, fixture).await?
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(receipt.version == 1, "unsupported SQLite fixture receipt");
    ensure!(
        receipt.resource_id == claim.resource_id
            && receipt.physical_identity == claim.physical_identity,
        "SQLite fixture receipt belongs to another resource"
    );
    let source_hash = hash_regular_file(fixture)?;
    ensure!(
        source_hash == receipt.fixture_hash,
        "SQLite fixture changed since its frozen receipt"
    );
    let frozen = resource_dir
        .join(format!("generation-{}", receipt.generation))
        .join("frozen")
        .join(DATABASE_FILE);
    ensure!(
        frozen.starts_with(resource_dir),
        "invalid frozen SQLite fixture target"
    );
    validate_regular_file(&frozen)?;
    ensure!(
        hash_regular_file(&frozen)? == receipt.fixture_hash,
        "frozen SQLite fixture does not match its receipt"
    );
    validate_sqlite_delete(&frozen).await?;
    // A reader must use the frozen generation declared by the write receipt, not a mutable
    // database produced by a previous write check. Its own step generation is still represented
    // in the mount environment hash for evidence binding.
    Ok(DbMount {
        host_path: frozen
            .parent()
            .context("frozen SQLite fixture has no directory")?
            .to_path_buf(),
        container_path: container_mount_path(claim),
        database_path: container_mount_path(claim).join(DATABASE_FILE),
        read_only: true,
        environment_hash: environment_hash(claim, &receipt.fixture_hash, receipt.generation, true),
        generation: receipt.generation,
    })
}

/// A first shared reader may establish a read-only seed, but it never changes a resource directory
/// already visible to another reader or writer. The completed directory and receipt appear in one
/// rename, so concurrent first readers either publish once or consume the winner's exact receipt.
async fn publish_first_read_seed(
    root: &Path,
    resource_dir: &Path,
    claim: &ResourceClaimView,
    fixture: &Path,
) -> Result<FixtureReceipt> {
    ensure!(
        resource_dir.starts_with(root),
        "invalid private SQLite fixture target"
    );
    let counter = SEED_STAGING.fetch_add(1, Ordering::Relaxed);
    let staging = root.join(format!(
        ".fixture-seed-{}-{}-{counter}",
        hash_bytes(claim.physical_identity.as_bytes()),
        std::process::id()
    ));
    fs::create_dir(&staging).context("create SQLite seed staging directory")?;
    let result = async {
        let generation_dir = staging.join("generation-0");
        let frozen_dir = generation_dir.join("frozen");
        fs::create_dir(&generation_dir)?;
        fs::create_dir(&frozen_dir)?;
        let frozen = frozen_dir.join(DATABASE_FILE);
        let fixture_hash = copy_fixture(fixture, &[&frozen])?;
        validate_sqlite_delete(&frozen).await?;
        let receipt = FixtureReceipt {
            version: 1,
            resource_id: claim.resource_id.clone(),
            physical_identity: claim.physical_identity.clone(),
            fixture_hash,
            generation: 0,
        };
        write_receipt(&staging.join(RECEIPT_FILE), &receipt)?;
        sync_dir(&frozen_dir)?;
        sync_dir(&generation_dir)?;
        sync_dir(&staging)?;
        Ok::<FixtureReceipt, anyhow::Error>(receipt)
    }
    .await;
    let receipt = match result {
        Ok(receipt) => receipt,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    match fs::rename(&staging, resource_dir) {
        Ok(()) => {
            sync_dir(root)?;
            Ok(receipt)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            let _ = fs::remove_dir_all(&staging);
            read_receipt(&resource_dir.join(RECEIPT_FILE))
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error.into())
        }
    }
}

fn private_shared_db_root(private_root: &Path) -> Result<PathBuf> {
    ensure!(
        private_root.is_absolute(),
        "workflow private root must be absolute"
    );
    fs::create_dir_all(private_root)?;
    let private_root = private_root.canonicalize()?;
    ensure!(
        private_root != Path::new("/"),
        "workflow private root cannot be filesystem root"
    );
    let root = private_root.join("shared-db");
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    ensure!(
        root.starts_with(&private_root),
        "invalid workflow shared-db root"
    );
    Ok(root)
}

async fn validate_fixture(path: &Path) -> Result<PathBuf> {
    let path = absolute_file(path)?;
    validate_regular_file(&path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let auxiliary = PathBuf::from(format!("{}{}", path.display(), suffix));
        if fs::symlink_metadata(&auxiliary).is_ok() {
            bail!("SQLite fixture has an active {suffix} sidecar");
        }
    }
    validate_sqlite_delete(&path).await?;
    Ok(path)
}

fn absolute_file(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "SQLite fixture path must be absolute");
    Ok(path.to_path_buf())
}

fn validate_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("read SQLite fixture metadata: {}", path.display()))?;
    ensure!(
        !metadata.file_type().is_symlink() && metadata.is_file(),
        "SQLite fixture must be a regular non-symlink file"
    );
    ensure!(
        metadata.len() > 0 && metadata.len() <= MAX_FIXTURE_BYTES,
        "SQLite fixture exceeds its size limit"
    );
    Ok(())
}

async fn validate_sqlite_delete(path: &Path) -> Result<()> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false);
    let pool: SqlitePool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await?;
    pool.close().await;
    ensure!(
        mode.eq_ignore_ascii_case("delete"),
        "SQLite fixture must use journal_mode=DELETE"
    );
    Ok(())
}

/// Copies the seed into one or more private targets from one bounded input stream, producing the
/// exact hash later fenced by every reader receipt.
fn copy_fixture(source: &Path, targets: &[&Path]) -> Result<String> {
    validate_regular_file(source)?;
    ensure!(
        !targets.is_empty(),
        "SQLite fixture copy has no destination"
    );
    let mut source = File::open(source)?;
    let mut outputs = targets
        .iter()
        .map(|target| OpenOptions::new().write(true).create_new(true).open(target))
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .context("SQLite fixture size overflow")?;
        ensure!(
            total <= MAX_FIXTURE_BYTES,
            "SQLite fixture exceeds its size limit"
        );
        digest.update(&buffer[..count]);
        for output in &mut outputs {
            output.write_all(&buffer[..count])?;
        }
    }
    for output in outputs {
        output.sync_all()?;
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn hash_regular_file(path: &Path) -> Result<String> {
    validate_regular_file(path)?;
    let mut input = File::open(path)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .context("SQLite fixture size overflow")?;
        ensure!(
            total <= MAX_FIXTURE_BYTES,
            "SQLite fixture exceeds its size limit"
        );
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_receipt(path: &Path, receipt: &FixtureReceipt) -> Result<()> {
    let parent = path
        .parent()
        .context("SQLite fixture receipt has no parent")?;
    let temporary = parent.join("fixture-receipt.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec(receipt)?)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    sync_dir(parent)
}

fn read_receipt(path: &Path) -> Result<FixtureReceipt> {
    validate_regular_file(path)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn environment_hash(
    claim: &ResourceClaimView,
    fixture_hash: &str,
    generation: i64,
    read_only: bool,
) -> String {
    hash_bytes(
        format!(
            "sqlite-fixture-v1\0{}\0{}\0{}\0{}\0{}",
            claim.resource_id, claim.physical_identity, fixture_hash, generation, read_only
        )
        .as_bytes(),
    )
}

fn container_mount_path(claim: &ResourceClaimView) -> PathBuf {
    PathBuf::from("/praxis/test-db").join(hash_bytes(claim.physical_identity.as_bytes()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
