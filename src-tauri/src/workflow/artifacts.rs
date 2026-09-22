//! Immutable, content-addressed workflow snapshots.  This module never reads
//! or writes a Git checkout; callers provide fixed input and output trees.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod composition;
mod git_input;
mod process;
pub use composition::{AncestorArtifact, ComposedInput};
pub use git_input::{GitInputPolicy, GitInputReceipt};

pub const DEFAULT_MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024 * 1024;
pub const DEFAULT_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct EntryState {
    pub kind: EntryKind,
    pub mode: u32,
    pub size: u64,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub path: String,
    #[serde(flatten)]
    pub state: EntryState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeManifest {
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaOperation {
    Add,
    Modify,
    Delete,
    ReplaceType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeltaEntry {
    pub path: String,
    pub operation: DeltaOperation,
    pub before: Option<EntryState>,
    pub after: Option<EntryState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub entries: Vec<DeltaEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub artifact_id: String,
    pub parent_input_hash: String,
    pub output_tree_hash: String,
    pub delta_hash: String,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CapturePolicy {
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub write_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactLimits {
    pub max_snapshot_bytes: u64,
    pub max_file_bytes: u64,
}

impl Default for ArtifactLimits {
    fn default() -> Self {
        Self {
            max_snapshot_bytes: DEFAULT_MAX_SNAPSHOT_BYTES,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
    limits: ArtifactLimits,
}

impl TreeManifest {
    pub fn hash(&self) -> Result<String> {
        self.validate()?;
        Ok(hash_bytes(&serde_json::to_vec(self)?))
    }

    pub fn validate(&self) -> Result<()> {
        let mut previous: Option<&[u8]> = None;
        let mut states = BTreeMap::new();
        for entry in &self.entries {
            validate_relative_path(&entry.path)?;
            if previous.is_some_and(|value| value >= entry.path.as_bytes()) {
                bail!("manifest entries must be unique and bytewise sorted");
            }
            validate_state(&entry.state)?;
            states.insert(entry.path.as_str(), &entry.state);
            previous = Some(entry.path.as_bytes());
        }
        for entry in &self.entries {
            let mut parent = Path::new(&entry.path).parent();
            while let Some(path) = parent {
                if path.as_os_str().is_empty() {
                    break;
                }
                let parent_key = path
                    .to_str()
                    .ok_or_else(|| anyhow!("snapshot paths must be UTF-8"))?;
                if states
                    .get(parent_key)
                    .is_none_or(|state| state.kind != EntryKind::Directory)
                {
                    bail!("snapshot entry has no directory parent: {}", entry.path);
                }
                parent = path.parent();
            }
        }
        Ok(())
    }

    pub fn states(&self) -> BTreeMap<String, EntryState> {
        self.entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.state.clone()))
            .collect()
    }

    pub fn from_states(states: BTreeMap<String, EntryState>) -> Result<Self> {
        let manifest = Self {
            entries: states
                .into_iter()
                .map(|(path, state)| ManifestEntry { path, state })
                .collect(),
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

impl Delta {
    pub fn hash(&self) -> Result<String> {
        self.validate()?;
        Ok(hash_bytes(&serde_json::to_vec(self)?))
    }

    pub fn validate(&self) -> Result<()> {
        let mut previous: Option<&[u8]> = None;
        for entry in &self.entries {
            validate_relative_path(&entry.path)?;
            if previous.is_some_and(|value| value >= entry.path.as_bytes()) {
                bail!("delta entries must be unique and bytewise sorted");
            }
            match entry.operation {
                DeltaOperation::Add if entry.before.is_some() || entry.after.is_none() => {
                    bail!("add delta must have only an after state")
                }
                DeltaOperation::Delete if entry.before.is_none() || entry.after.is_some() => {
                    bail!("delete delta must have a before tombstone and no after state")
                }
                DeltaOperation::Modify if entry.before.is_none() || entry.after.is_none() => {
                    bail!("modify delta must have before and after states")
                }
                DeltaOperation::ReplaceType if entry.before.is_none() || entry.after.is_none() => {
                    bail!("replace_type delta must have before and after states")
                }
                _ => {}
            }
            if let Some(before) = &entry.before {
                validate_state(before)?;
            }
            if let Some(after) = &entry.after {
                validate_state(after)?;
            }
            if matches!(entry.operation, DeltaOperation::ReplaceType)
                && entry.before.as_ref().unwrap().kind == entry.after.as_ref().unwrap().kind
            {
                bail!("replace_type delta must change entry kind");
            }
            previous = Some(entry.path.as_bytes());
        }
        Ok(())
    }
}

impl ArtifactStore {
    /// `root` is a Runner-owned private directory. Caller-supplied snapshots
    /// are never resolved beneath it and are traversed through no-follow FDs.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_limits(root, ArtifactLimits::default())
    }

    pub fn open_with_limits(root: impl AsRef<Path>, limits: ArtifactLimits) -> Result<Self> {
        if limits.max_snapshot_bytes == 0 || limits.max_file_bytes == 0 {
            bail!("artifact limits must be non-zero");
        }
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(root.join("artifacts"))?;
        fs::create_dir_all(root.join("inputs"))?;
        fs::create_dir_all(root.join("trees"))?;
        sync_directory(&root.join("artifacts"))?;
        sync_directory(&root.join("inputs"))?;
        sync_directory(&root.join("trees"))?;
        Ok(Self { root, limits })
    }

    /// Captures an output tree against a caller-supplied, fixed input tree.
    /// The input root is read only and neither tree is ever modified.
    pub fn capture(
        &self,
        parent_input_tree: impl AsRef<Path>,
        output_tree: impl AsRef<Path>,
        policy: &CapturePolicy,
    ) -> Result<Artifact> {
        validate_policy(policy)?;
        let input = scan_tree(parent_input_tree.as_ref(), self.limits)?;
        let output = scan_tree(output_tree.as_ref(), self.limits)?;
        for entry in input.entries.iter().chain(&output.entries) {
            if git_input::credential_path(&entry.path) {
                bail!(
                    "snapshot preflight rejects credential or repository metadata path: {}",
                    entry.path
                );
            }
        }
        let delta = canonical_delta(&input, &output)?;
        enforce_scope(&delta, policy)?;
        let parent_input_hash = input.hash()?;
        let output_tree_hash = output.hash()?;
        let delta_hash = delta.hash()?;
        let artifact = Artifact {
            artifact_id: artifact_id(&parent_input_hash, &output_tree_hash, &delta_hash)?,
            parent_input_hash,
            output_tree_hash: output_tree_hash.clone(),
            delta_hash,
            manifest_hash: output_tree_hash,
        };
        self.publish_input_tree(
            parent_input_tree.as_ref(),
            &input,
            &artifact.parent_input_hash,
        )?;
        self.publish(output_tree.as_ref(), &output, &delta, &artifact)?;
        Ok(artifact)
    }

    pub fn load(&self, requested_artifact_id: &str) -> Result<(Artifact, TreeManifest, Delta)> {
        validate_hash(requested_artifact_id)?;
        let directory = self.root.join("artifacts").join(requested_artifact_id);
        let artifact: Artifact = read_json(directory.join("artifact.json"))?;
        if artifact.artifact_id != requested_artifact_id {
            bail!("stored artifact directory does not match artifact hash");
        }
        if artifact.artifact_id
            != artifact_id(
                &artifact.parent_input_hash,
                &artifact.output_tree_hash,
                &artifact.delta_hash,
            )?
        {
            bail!("stored artifact receipt hash does not match its immutable inputs");
        }
        validate_hash(&artifact.parent_input_hash)?;
        validate_hash(&artifact.output_tree_hash)?;
        validate_hash(&artifact.delta_hash)?;
        validate_hash(&artifact.manifest_hash)?;
        if artifact.manifest_hash != artifact.output_tree_hash {
            bail!("artifact manifest hash must equal its output tree hash");
        }
        let delta: Delta = read_json(directory.join("delta.json"))?;
        if delta.hash()? != artifact.delta_hash {
            bail!("stored delta hash does not match artifact receipt");
        }
        let parent = self.load_input_tree(&artifact.parent_input_hash)?;
        let output = self.load_tree(&artifact.output_tree_hash)?;
        if canonical_delta(&parent, &output)? != delta {
            bail!("stored delta does not reconstruct from the stored parent and output trees");
        }
        Ok((artifact, output, delta))
    }

    /// Looks up and verifies the reusable immutable full-tree blob by hash.
    pub fn load_tree(&self, output_tree_hash: &str) -> Result<TreeManifest> {
        self.load_snapshot("trees", output_tree_hash)
    }

    /// Looks up and verifies the immutable input tree bound to an artifact.
    pub fn load_input_tree(&self, parent_input_hash: &str) -> Result<TreeManifest> {
        self.load_snapshot("inputs", parent_input_hash)
    }

    fn load_snapshot(&self, collection: &str, tree_hash: &str) -> Result<TreeManifest> {
        validate_hash(tree_hash)?;
        let directory = self.root.join(collection).join(tree_hash);
        let manifest: TreeManifest = read_json(directory.join("manifest.json"))?;
        if manifest.hash()? != tree_hash {
            bail!("stored tree manifest hash does not match its directory");
        }
        let copied = scan_tree(&directory.join("files"), self.limits)?;
        if copied != manifest {
            bail!("stored file tree does not match stored manifest");
        }
        Ok(manifest)
    }

    /// Copies an immutable stored snapshot into a new caller-owned directory.
    /// It deliberately creates fresh files and never uses hardlinks or Git alternates.
    pub fn materialize(&self, artifact: &Artifact, destination: impl AsRef<Path>) -> Result<()> {
        let (loaded, manifest, _) = self.load(&artifact.artifact_id)?;
        if &loaded != artifact {
            bail!("artifact receipt does not match stored content");
        }
        let destination = destination.as_ref();
        if destination.exists() {
            bail!("materialization destination already exists");
        }
        fs::create_dir(destination)?;
        let source = self
            .root
            .join("trees")
            .join(&artifact.output_tree_hash)
            .join("files");
        if let Err(error) = copy_manifest_tree(&source, destination, &manifest) {
            let _ = fs::remove_dir_all(destination);
            return Err(error);
        }
        if scan_tree(destination, self.limits)? != manifest {
            let _ = fs::remove_dir_all(destination);
            bail!("materialized tree does not match its manifest");
        }
        sync_directory(destination)?;
        Ok(())
    }

    fn publish(
        &self,
        output_tree: &Path,
        manifest: &TreeManifest,
        delta: &Delta,
        artifact: &Artifact,
    ) -> Result<()> {
        self.publish_tree(output_tree, manifest, &artifact.output_tree_hash)?;
        let artifacts = self.root.join("artifacts");
        let target = artifacts.join(&artifact.artifact_id);
        if target.exists() {
            let (existing, existing_manifest, existing_delta) = self.load(&artifact.artifact_id)?;
            if existing == *artifact && existing_manifest == *manifest && existing_delta == *delta {
                return Ok(());
            }
            bail!("artifact hash already exists with different content");
        }
        let stage = self.root.join(format!(
            ".artifact-stage-{}-{}",
            std::process::id(),
            STAGE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&stage)?;
        let result = (|| {
            write_json(&stage.join("delta.json"), delta)?;
            write_json(&stage.join("artifact.json"), artifact)?;
            sync_directory(&stage)?;
            publish_directory(&stage, &target, || {
                let (stored, stored_manifest, stored_delta) = self.load(&artifact.artifact_id)?;
                if stored != *artifact || stored_manifest != *manifest || stored_delta != *delta {
                    bail!("concurrent artifact publication has different content");
                }
                Ok(())
            })?;
            sync_directory(&artifacts)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }

    fn publish_tree(
        &self,
        output_tree: &Path,
        manifest: &TreeManifest,
        tree_hash: &str,
    ) -> Result<()> {
        self.publish_snapshot(output_tree, manifest, tree_hash, "trees")
    }

    fn publish_input_tree(
        &self,
        input_tree: &Path,
        manifest: &TreeManifest,
        tree_hash: &str,
    ) -> Result<()> {
        self.publish_snapshot(input_tree, manifest, tree_hash, "inputs")
    }

    fn publish_snapshot(
        &self,
        source_tree: &Path,
        manifest: &TreeManifest,
        tree_hash: &str,
        collection: &str,
    ) -> Result<()> {
        let trees = self.root.join(collection);
        let target = trees.join(tree_hash);
        if target.exists() {
            if self.load_snapshot(collection, tree_hash)? == *manifest {
                return Ok(());
            }
            bail!("tree hash already exists with different content");
        }
        let stage = self.root.join(format!(
            ".tree-stage-{}-{}",
            std::process::id(),
            STAGE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&stage)?;
        let result = (|| {
            let files = stage.join("files");
            fs::create_dir(&files)?;
            copy_manifest_tree(source_tree, &files, manifest)?;
            if scan_tree(&files, self.limits)? != *manifest {
                bail!("published tree does not match its manifest");
            }
            write_json(&stage.join("manifest.json"), manifest)?;
            sync_directory(&files)?;
            sync_directory(&stage)?;
            publish_directory(&stage, &target, || {
                if self.load_snapshot(collection, tree_hash)? != *manifest {
                    bail!("concurrent input publication has different content");
                }
                Ok(())
            })?;
            sync_directory(&trees)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }
}

/// Another publisher can win after the existence check. Its fully published
/// bytes must be verified before treating that collision as idempotent success.
fn publish_directory(
    stage: &Path,
    target: &Path,
    verify_existing: impl FnOnce() -> Result<()>,
) -> Result<()> {
    match fs::rename(stage, target) {
        Ok(()) => Ok(()),
        Err(_) if target.is_dir() => {
            verify_existing()?;
            fs::remove_dir_all(stage)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub fn canonical_delta(input: &TreeManifest, output: &TreeManifest) -> Result<Delta> {
    input.validate()?;
    output.validate()?;
    let input = input.states();
    let output = output.states();
    let mut paths = input.keys().chain(output.keys()).collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut entries = Vec::new();
    for path in paths {
        let before = input.get(path).cloned();
        let after = output.get(path).cloned();
        if before == after {
            continue;
        }
        let operation = match (&before, &after) {
            (None, Some(_)) => DeltaOperation::Add,
            (Some(_), None) => DeltaOperation::Delete,
            (Some(before), Some(after)) if before.kind != after.kind => DeltaOperation::ReplaceType,
            (Some(_), Some(_)) => DeltaOperation::Modify,
            (None, None) => unreachable!(),
        };
        entries.push(DeltaEntry {
            path: path.clone(),
            operation,
            before,
            after,
        });
    }
    let delta = Delta { entries };
    delta.validate()?;
    Ok(delta)
}

#[cfg(unix)]
fn scan_tree(root: &Path, limits: ArtifactLimits) -> Result<TreeManifest> {
    let directory = open_directory_no_follow(root)?;
    let mut entries = Vec::new();
    let mut total = 0_u64;
    scan_directory_fd(&directory, "", &mut entries, &mut total, limits)?;
    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    let manifest = TreeManifest { entries };
    manifest.validate()?;
    Ok(manifest)
}

#[cfg(not(unix))]
fn scan_tree(_root: &Path, _limits: ArtifactLimits) -> Result<TreeManifest> {
    bail!("workflow artifact capture requires Unix no-follow directory traversal")
}

#[cfg(unix)]
fn scan_directory_fd(
    directory: &File,
    prefix: &str,
    entries: &mut Vec<ManifestEntry>,
    total: &mut u64,
    limits: ArtifactLimits,
) -> Result<()> {
    for name in directory_names(directory)? {
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let child = open_child_no_follow(directory, &name, false)?;
        let metadata = child.metadata()?;
        if metadata.is_dir() {
            let directory_path = relative.clone();
            entries.push(ManifestEntry {
                path: relative,
                state: directory_state(),
            });
            scan_directory_fd(&child, &directory_path, entries, total, limits)?;
        } else if metadata.is_file() {
            if metadata.len() > limits.max_file_bytes {
                bail!("snapshot file exceeds maximum size: {relative}");
            }
            *total = total
                .checked_add(metadata.len())
                .ok_or_else(|| anyhow!("snapshot size overflow"))?;
            if *total > limits.max_snapshot_bytes {
                bail!("snapshot exceeds maximum size");
            }
            entries.push(ManifestEntry {
                path: relative,
                state: EntryState {
                    kind: EntryKind::File,
                    mode: normalized_file_mode(&metadata),
                    size: metadata.len(),
                    hash: hash_open_file(&child, metadata.len())?,
                },
            });
        } else {
            bail!("special files are not supported in snapshots: {relative}");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_no_follow(path: &Path) -> Result<File> {
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, io::FromRawFd},
    };

    let path = CString::new(path.as_os_str().as_bytes())?;
    let fd = unsafe {
        nix::libc::open(
            path.as_ptr(),
            nix::libc::O_RDONLY
                | nix::libc::O_DIRECTORY
                | nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // A successful open transfers this owned descriptor to File, which closes it.
    let file = unsafe { File::from_raw_fd(fd) };
    if !file.metadata()?.is_dir() {
        bail!("snapshot root must be a real directory");
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_directory_no_follow(_path: &Path) -> Result<File> {
    bail!("workflow artifact capture requires Unix no-follow directory traversal")
}

#[cfg(unix)]
fn open_child_no_follow(parent: &File, name: &str, directory_only: bool) -> Result<File> {
    use std::{
        ffi::CString,
        os::unix::io::{AsRawFd, FromRawFd},
    };

    validate_relative_path(name)?;
    let name = CString::new(name)?;
    let mut stat: nix::libc::stat = unsafe { std::mem::zeroed() };
    if unsafe {
        nix::libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            nix::libc::AT_SYMLINK_NOFOLLOW,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let kind = (stat.st_mode as nix::libc::mode_t) & nix::libc::S_IFMT;
    if kind == nix::libc::S_IFLNK {
        bail!("symbolic links are not supported in snapshots: {name:?}");
    }
    if kind != nix::libc::S_IFREG && kind != nix::libc::S_IFDIR {
        bail!("special files are not supported in snapshots: {name:?}");
    }
    if directory_only && kind != nix::libc::S_IFDIR {
        bail!("snapshot path component is not a directory: {name:?}");
    }
    let flags = nix::libc::O_RDONLY
        | nix::libc::O_NOFOLLOW
        | nix::libc::O_CLOEXEC
        | nix::libc::O_NONBLOCK
        | if directory_only {
            nix::libc::O_DIRECTORY
        } else {
            0
        };
    let fd = unsafe { nix::libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(nix::libc::ELOOP) {
            bail!("symbolic links are not supported in snapshots: {name:?}");
        }
        return Err(error.into());
    }
    // The File owns this exact openat result; no borrowed raw FD escapes.
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn open_child_no_follow(_parent: &File, _name: &str, _directory_only: bool) -> Result<File> {
    bail!("workflow artifact capture requires Unix no-follow directory traversal")
}

#[cfg(unix)]
fn directory_names(directory: &File) -> Result<Vec<String>> {
    use std::{ffi::CStr, os::unix::io::AsRawFd};

    let fd = unsafe { nix::libc::dup(directory.as_raw_fd()) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let stream = unsafe { nix::libc::fdopendir(fd) };
    if stream.is_null() {
        unsafe { nix::libc::close(fd) };
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| {
        let mut names = Vec::new();
        loop {
            clear_errno();
            let entry = unsafe { nix::libc::readdir(stream) };
            if entry.is_null() {
                let error = last_errno();
                if error != 0 {
                    return Err(std::io::Error::from_raw_os_error(error).into());
                }
                break;
            }
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            let name = std::str::from_utf8(bytes)
                .map_err(|_| anyhow!("snapshot paths must be UTF-8"))?
                .to_owned();
            validate_relative_path(&name)?;
            names.push(name);
        }
        names.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        Ok(names)
    })();
    unsafe { nix::libc::closedir(stream) };
    result
}

#[cfg(target_os = "linux")]
fn clear_errno() {
    unsafe {
        *nix::libc::__errno_location() = 0;
    }
}
#[cfg(target_os = "linux")]
fn last_errno() -> i32 {
    unsafe { *nix::libc::__errno_location() }
}
#[cfg(target_os = "macos")]
fn clear_errno() {
    unsafe {
        *nix::libc::__error() = 0;
    }
}
#[cfg(target_os = "macos")]
fn last_errno() -> i32 {
    unsafe { *nix::libc::__error() }
}
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn clear_errno() {}
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn last_errno() -> i32 {
    0
}

#[cfg(unix)]
fn open_relative_file_no_follow(root: &File, path: &str) -> Result<File> {
    validate_relative_path(path)?;
    let mut directory = root.try_clone()?;
    let mut components = path.split('/').peekable();
    while let Some(component) = components.next() {
        let last = components.peek().is_none();
        let child = open_child_no_follow(&directory, component, !last)?;
        if last {
            return Ok(child);
        }
        directory = child;
    }
    unreachable!("validated paths have a component")
}

#[cfg(not(unix))]
fn open_relative_file_no_follow(_root: &File, _path: &str) -> Result<File> {
    bail!("workflow artifact copy requires Unix no-follow directory traversal")
}

#[cfg(unix)]
fn hash_open_file(file: &File, expected_size: u64) -> Result<String> {
    let mut file = file.try_clone()?;
    let maximum = expected_size
        .checked_add(1)
        .ok_or_else(|| anyhow!("snapshot file size overflow"))?;
    let mut bounded = Read::by_ref(&mut file).take(maximum);
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut observed = 0_u64;
    loop {
        let count = bounded.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        observed = observed
            .checked_add(count as u64)
            .ok_or_else(|| anyhow!("snapshot file size overflow"))?;
        hash.update(&buffer[..count]);
    }
    if observed != expected_size {
        bail!("snapshot source changed while hashing");
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_manifest_tree(source: &Path, destination: &Path, manifest: &TreeManifest) -> Result<()> {
    manifest.validate()?;
    let source_root = open_directory_no_follow(source)?;
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| entry.state.kind == EntryKind::Directory)
    {
        let target = destination.join(&entry.path);
        fs::create_dir(&target)?;
        set_mode(&target, entry.state.mode)?;
    }
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| entry.state.kind == EntryKind::File)
    {
        let mut input = open_relative_file_no_follow(&source_root, &entry.path)?;
        let metadata = input.metadata()?;
        if !metadata.is_file() || metadata.len() != entry.state.size {
            bail!("snapshot source changed while copying: {}", entry.path);
        }
        let target = destination.join(&entry.path);
        let mut output = create_file(&target)?;
        let max_copy = entry
            .state
            .size
            .checked_add(1)
            .ok_or_else(|| anyhow!("snapshot file size overflow"))?;
        let mut bounded = Read::by_ref(&mut input).take(max_copy);
        let copied = std::io::copy(&mut bounded, &mut output)?;
        if copied != entry.state.size {
            bail!("snapshot source changed while copying: {}", entry.path);
        }
        set_mode(&target, entry.state.mode)?;
        output.sync_all()?;
        drop(output);
        if hash_file(&target)? != entry.state.hash {
            bail!("snapshot source changed while copying: {}", entry.path);
        }
    }
    sync_tree(destination, manifest)
}

fn sync_tree(root: &Path, manifest: &TreeManifest) -> Result<()> {
    for entry in manifest
        .entries
        .iter()
        .rev()
        .filter(|entry| entry.state.kind == EntryKind::Directory)
    {
        sync_directory(&root.join(&entry.path))?;
    }
    sync_directory(root)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let mut file = create_file(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Result<T> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn create_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn artifact_id(
    parent_input_hash: &str,
    output_tree_hash: &str,
    delta_hash: &str,
) -> Result<String> {
    validate_hash(parent_input_hash)?;
    validate_hash(output_tree_hash)?;
    validate_hash(delta_hash)?;
    Ok(hash_bytes(
        format!("{parent_input_hash}\0{output_tree_hash}\0{delta_hash}").as_bytes(),
    ))
}

fn directory_state() -> EntryState {
    EntryState {
        kind: EntryKind::Directory,
        mode: 0o755,
        size: 0,
        hash: hash_bytes(&[]),
    }
}

fn normalized_file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            0o755
        } else {
            0o644
        }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

fn validate_state(state: &EntryState) -> Result<()> {
    let valid_mode = match state.kind {
        EntryKind::File => matches!(state.mode, 0o644 | 0o755),
        EntryKind::Directory => state.mode == 0o755,
    };
    if !valid_mode {
        bail!("snapshot entry has unsupported mode");
    }
    if state.kind == EntryKind::Directory && state.size != 0 {
        bail!("directory snapshot entry must have zero size");
    }
    validate_hash(&state.hash)
}

fn validate_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        bail!("expected lowercase SHA-256 hash");
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<()> {
    if value.chars().any(char::is_control) {
        bail!("invalid relative snapshot path");
    }
    if value.len() > 4096 || value.contains('\\') {
        bail!("invalid relative snapshot path");
    }
    super::policy::validate_safe_path(value).map_err(anyhow::Error::msg)
}

fn validate_policy(policy: &CapturePolicy) -> Result<()> {
    for path in policy
        .include_paths
        .iter()
        .chain(&policy.exclude_paths)
        .chain(&policy.write_paths)
    {
        validate_relative_path(path)?;
    }
    Ok(())
}

fn enforce_scope(delta: &Delta, policy: &CapturePolicy) -> Result<()> {
    for entry in &delta.entries {
        let included = policy.include_paths.is_empty()
            || policy
                .include_paths
                .iter()
                .any(|prefix| is_under(&entry.path, prefix));
        let excluded = policy
            .exclude_paths
            .iter()
            .any(|prefix| is_under(&entry.path, prefix));
        let writable = policy
            .write_paths
            .iter()
            .any(|prefix| is_under(&entry.path, prefix));
        if !included || excluded {
            bail!(
                "changed path is outside the output contract: {}",
                entry.path
            );
        }
        if !writable {
            bail!("changed path is outside task write_paths: {}", entry.path);
        }
    }
    Ok(())
}

fn is_under(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.starts_with('/'))
}

#[cfg(all(test, unix))]
#[path = "../../tests/support/temp_root.rs"]
mod test_temp_root;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn concurrent_publish_collision_verifies_the_winner_and_cleans_the_loser() {
        let root = test_temp_root::dir().join("artifact-publish-collision");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("published");
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for index in 0..2 {
                let stage = root.join(format!("stage-{index}"));
                fs::create_dir(&stage).unwrap();
                fs::write(stage.join("blob"), b"same").unwrap();
                let target = &target;
                let barrier = &barrier;
                handles.push(scope.spawn(move || {
                    // Both staged complete content before either publishes.
                    barrier.wait();
                    publish_directory(&stage, target, || {
                        if fs::read(target.join("blob"))? != b"same" {
                            bail!("different published content");
                        }
                        Ok(())
                    })
                    .unwrap();
                    assert!(!stage.exists());
                }));
            }
            for handle in handles {
                handle.join().unwrap();
            }
        });
        let mismatch = root.join("mismatch");
        fs::create_dir(&mismatch).unwrap();
        fs::write(mismatch.join("blob"), b"different").unwrap();
        assert!(publish_directory(&mismatch, &target, || bail!("mismatch")).is_err());
        assert_eq!(fs::read(target.join("blob")).unwrap(), b"same");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_replacement_cannot_redirect_an_opened_snapshot() {
        let root = test_temp_root::dir().join("artifact-directory-replacement");
        let output = root.join("output");
        let outside = root.join("outside");
        fs::create_dir_all(output.join("src")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(output.join("src/item"), b"inside").unwrap();
        fs::write(outside.join("item"), b"outside-secret").unwrap();
        let root_fd = open_directory_no_follow(&output).unwrap();
        let src_fd = open_child_no_follow(&root_fd, "src", true).unwrap();
        // Deterministically replace the checked directory before later reads.
        fs::rename(output.join("src"), output.join("saved")).unwrap();
        symlink(&outside, output.join("src")).unwrap();
        let mut entries = Vec::new();
        scan_directory_fd(
            &src_fd,
            "src",
            &mut entries,
            &mut 0,
            ArtifactLimits::default(),
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state.hash, hash_bytes(b"inside"));
        assert!(open_relative_file_no_follow(&root_fd, "src/item").is_err());
        drop(src_fd);
        drop(root_fd);
        fs::remove_dir_all(root).unwrap();
    }
}
