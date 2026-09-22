use std::{collections::BTreeMap, fs, io::Read, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::{process, ArtifactStore, EntryKind, EntryState, TreeManifest};
use crate::workflow::inputs::{merge_with_content, AncestorDelta};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AncestorArtifact {
    pub node_id: String,
    pub topo_order: u32,
    pub artifact_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposedInput {
    /// The actual combined tree hash, separate from the scheduler fingerprint.
    pub input_tree_hash: String,
    pub manifest: TreeManifest,
    pub applied_ancestors: Vec<String>,
}

impl ArtifactStore {
    /// Compose verified ancestor receipts in stable topological order. The
    /// caller selects the required ancestry; this function checks every blob
    /// and receipt, but does not assert a worker result has passed verification.
    pub fn compose_input(
        &self,
        base_input_hash: &str,
        ancestors: Vec<AncestorArtifact>,
    ) -> Result<ComposedInput> {
        let base = self.load_input_tree(base_input_hash)?;
        let stage = process::Stage::new(&self.root)?;
        let mut blobs = BTreeMap::<String, (PathBuf, String)>::new();
        register_blobs(
            &mut blobs,
            &base,
            self.root.join("inputs").join(base_input_hash).join("files"),
        );
        let mut deltas = Vec::new();
        let mut identities = BTreeMap::new();
        for ancestor in ancestors {
            if let Some(previous) = identities.insert(ancestor.node_id.clone(), ancestor.clone()) {
                if previous != ancestor {
                    bail!("ancestor has conflicting artifact receipts");
                }
                continue;
            }
            let (artifact, output, delta) = self.load(&ancestor.artifact_id)?;
            let parent = self.load_input_tree(&artifact.parent_input_hash)?;
            register_blobs(
                &mut blobs,
                &parent,
                self.root
                    .join("inputs")
                    .join(&artifact.parent_input_hash)
                    .join("files"),
            );
            register_blobs(
                &mut blobs,
                &output,
                self.root
                    .join("trees")
                    .join(&artifact.output_tree_hash)
                    .join("files"),
            );
            deltas.push(AncestorDelta {
                node_id: ancestor.node_id,
                topo_order: ancestor.topo_order,
                delta,
            });
        }
        let mut merge_no = 0;
        let merged = merge_with_content(&base, deltas, |path, before, ours, theirs| {
            merge_no += 1;
            let directory = stage.0.join(format!("merge-{merge_no}"));
            fs::create_dir(&directory)?;
            for (name, state) in [("base", before), ("ours", ours), ("theirs", theirs)] {
                let bytes = read_blob(&blobs, state, self.limits.max_file_bytes)?;
                if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                    bail!("input_conflict at {path}: binary content changed on both branches");
                }
                fs::write(directory.join(name), bytes)?;
            }
            let output = directory.join("merged");
            let status = process::git(
                &directory,
                &[
                    "merge-file",
                    "-p",
                    "--diff3",
                    "--",
                    "ours",
                    "base",
                    "theirs",
                ],
                &output,
                self.limits.max_file_bytes,
            )?;
            if !status.success() {
                // Preserve exact three-way inputs under a deterministic conflict
                // receipt. They are private Runner artifacts, never source edits.
                let key = super::hash_bytes(&serde_json::to_vec(&(path, before, ours, theirs))?);
                let conflicts = self.root.join("conflicts");
                fs::create_dir_all(&conflicts)?;
                let target = conflicts.join(&key);
                if !target.exists() {
                    for name in ["base", "ours", "theirs", "merged"] {
                        fs::File::open(directory.join(name))?.sync_all()?;
                    }
                    super::sync_directory(&directory)?;
                    fs::rename(&directory, &target)?;
                    super::sync_directory(&conflicts)?;
                }
                bail!("input_conflict at {path}: conflict receipt {key}");
            }
            let bytes = process::read_bounded(&output, self.limits.max_file_bytes)?;
            let state = EntryState {
                kind: EntryKind::File,
                mode: ours.mode,
                size: bytes.len() as u64,
                hash: super::hash_bytes(&bytes),
            };
            blobs.insert(state.hash.clone(), (directory, "merged".into()));
            Ok(state)
        })?;
        let files = stage.0.join("files");
        fs::create_dir(&files)?;
        let mut total = 0_u64;
        for entry in &merged.manifest.entries {
            let target = files.join(&entry.path);
            match entry.state.kind {
                EntryKind::Directory => fs::create_dir(&target)?,
                EntryKind::File => {
                    total = total
                        .checked_add(entry.state.size)
                        .context("input size overflow")?;
                    if total > self.limits.max_snapshot_bytes {
                        bail!("composed input exceeds snapshot limit");
                    }
                    let bytes = read_blob(&blobs, &entry.state, self.limits.max_file_bytes)?;
                    fs::write(&target, bytes)?;
                }
            }
            super::set_mode(&target, entry.state.mode)?;
        }
        let input_tree_hash = merged.manifest.hash()?;
        self.publish_input_tree(&files, &merged.manifest, &input_tree_hash)?;
        Ok(ComposedInput {
            input_tree_hash,
            manifest: merged.manifest,
            applied_ancestors: merged.applied_ancestors,
        })
    }

    pub fn materialize_input(
        &self,
        input_tree_hash: &str,
        destination: impl AsRef<std::path::Path>,
    ) -> Result<()> {
        let manifest = self.load_input_tree(input_tree_hash)?;
        let destination = destination.as_ref();
        fs::create_dir(destination)?;
        let source = self.root.join("inputs").join(input_tree_hash).join("files");
        let result = (|| {
            super::copy_manifest_tree(&source, destination, &manifest)?;
            if super::scan_tree(destination, self.limits)? != manifest {
                bail!("materialized input changed");
            }
            super::sync_directory(destination)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(destination);
        }
        result
    }
}

fn register_blobs(
    blobs: &mut BTreeMap<String, (PathBuf, String)>,
    manifest: &TreeManifest,
    root: PathBuf,
) {
    for entry in &manifest.entries {
        if entry.state.kind == EntryKind::File {
            blobs.insert(entry.state.hash.clone(), (root.clone(), entry.path.clone()));
        }
    }
}

fn read_blob(
    blobs: &BTreeMap<String, (PathBuf, String)>,
    state: &EntryState,
    maximum: u64,
) -> Result<Vec<u8>> {
    if state.size > maximum {
        bail!("input blob exceeds maximum size");
    }
    let (root, path) = blobs.get(&state.hash).context("input blob unavailable")?;
    let directory = super::open_directory_no_follow(root)?;
    let mut file =
        super::open_relative_file_no_follow(&directory, path)?.take(state.size.saturating_add(1));
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 != state.size || super::hash_bytes(&bytes) != state.hash {
        bail!("input blob changed while composing");
    }
    Ok(bytes)
}
