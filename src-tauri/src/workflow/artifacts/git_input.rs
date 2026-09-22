//! Export a pinned Git commit without checkout, filters, hooks, or credentials.
use std::{fs, path::Path};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::{process, ArtifactStore};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitInputPolicy {
    /// Normalized repository-relative prefixes supplied by the execution profile.
    /// Built-in credential/metadata exclusions always apply in addition.
    pub exclude_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInputReceipt {
    pub base_commit: String,
    pub input_tree_hash: String,
    pub filter_hash: String,
    pub excluded_paths: Vec<String>,
}

impl ArtifactStore {
    /// Only committed blobs are exported. Dirty/untracked files, Git metadata,
    /// LFS smudge filters and object replacement refs cannot affect the input.
    pub fn export_git_input(
        &self,
        repository: impl AsRef<Path>,
        base_commit: &str,
        policy: &GitInputPolicy,
    ) -> Result<GitInputReceipt> {
        if !matches!(base_commit.len(), 40 | 64)
            || !base_commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            bail!("base_commit must be an exact lowercase Git object ID");
        }
        for path in &policy.exclude_paths {
            super::validate_relative_path(path)?;
        }
        let mut excludes = policy.exclude_paths.clone();
        excludes.sort();
        excludes.dedup();
        let filter_hash = super::hash_bytes(&serde_json::to_vec(&("git-input-v1", &excludes))?);
        let repository = repository.as_ref().canonicalize()?;
        let stage = process::Stage::new(&self.root)?;
        let partial_config = stage.0.join("partial-clone-config");
        let partial_status = process::git(
            &repository,
            &[
                "config",
                "--local",
                "--get-regexp",
                "^(extensions\\.partialclone|remote\\..*\\.promisor)$",
            ],
            &partial_config,
            64 * 1024,
        )?;
        if partial_status.success() {
            bail!("input preflight rejects partial-clone/promisor repositories; input export must stay offline");
        }
        if partial_status.code() != Some(1) {
            bail!("cannot inspect repository input configuration");
        }
        let kind_path = stage.0.join("object-kind");
        if !process::git(
            &repository,
            &["cat-file", "-t", base_commit],
            &kind_path,
            64,
        )?
        .success()
            || process::read_bounded(&kind_path, 64)? != b"commit\n"
        {
            bail!("base_commit is not an available commit");
        }
        let listing = stage.0.join("entries");
        if !process::git(
            &repository,
            &["ls-tree", "-r", "-l", "-z", "--full-tree", base_commit],
            &listing,
            16 * 1024 * 1024,
        )?
        .success()
        {
            bail!("cannot enumerate pinned Git commit");
        }
        let listing_bytes = process::read_bounded(&listing, 16 * 1024 * 1024)?;
        let files = stage.0.join("files");
        fs::create_dir(&files)?;
        let mut total = 0_u64;
        let mut excluded_paths = Vec::new();
        for (index, record) in listing_bytes
            .split(|b| *b == 0)
            .filter(|r| !r.is_empty())
            .enumerate()
        {
            let (header, path) = record.split_at(
                record
                    .iter()
                    .position(|b| *b == b'\t')
                    .context("invalid Git tree record")?,
            );
            let path = std::str::from_utf8(&path[1..]).context("input paths must be UTF-8")?;
            super::validate_relative_path(path)?;
            let fields: Vec<_> = std::str::from_utf8(header)?.split_whitespace().collect();
            if fields.len() != 4 {
                bail!("invalid Git tree entry");
            }
            // Unsupported types fail preflight even when an exclusion matches.
            if !matches!(fields[0], "100644" | "100755") || fields[1] != "blob" {
                bail!("input preflight rejects symlink/submodule/special entry: {path}");
            }
            if credential_path(path) || excludes.iter().any(|prefix| super::is_under(path, prefix))
            {
                excluded_paths.push(path.to_owned());
                continue;
            }
            let size: u64 = fields[3].parse().context("invalid Git blob size")?;
            total = total.checked_add(size).context("input size overflow")?;
            if size > self.limits.max_file_bytes || total > self.limits.max_snapshot_bytes {
                bail!("Git input exceeds snapshot limit: {path}");
            }
            let oid = fields[2];
            if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("invalid Git blob ID");
            }
            let spool = stage.0.join(format!("blob-{index}"));
            if !process::git(&repository, &["cat-file", "blob", oid], &spool, size)?.success() {
                bail!("Git blob unavailable: {path}");
            }
            let bytes = process::read_bounded(&spool, size)?;
            if bytes.len() as u64 != size {
                bail!("Git blob size mismatch: {path}");
            }
            if bytes.starts_with(b"version https://git-lfs.github.com/spec/v1\n")
                || bytes.starts_with(b"version https://git-lfs.github.com/spec/v1\r\n")
            {
                bail!("input preflight rejects unhydrated LFS pointer: {path}");
            }
            let target = files.join(path);
            fs::create_dir_all(target.parent().context("input parent missing")?)?;
            fs::rename(&spool, &target)?;
            super::set_mode(&target, if fields[0] == "100755" { 0o755 } else { 0o644 })?;
        }
        let manifest = super::scan_tree(&files, self.limits)?;
        let input_tree_hash = manifest.hash()?;
        self.publish_input_tree(&files, &manifest, &input_tree_hash)?;
        Ok(GitInputReceipt {
            base_commit: base_commit.into(),
            input_tree_hash,
            filter_hash,
            excluded_paths,
        })
    }
}

pub(super) fn credential_path(path: &str) -> bool {
    path.split('/').any(|part| {
        let name = part.to_ascii_lowercase();
        matches!(
            name.as_str(),
            ".git"
                | ".ssh"
                | ".aws"
                | ".gnupg"
                | ".npmrc"
                | ".netrc"
                | "credentials"
                | "credentials.json"
                | "id_rsa"
                | "id_ed25519"
        ) || name == ".env"
            || (name.starts_with(".env.")
                && !matches!(
                    name.as_str(),
                    ".env.example" | ".env.sample" | ".env.template"
                ))
            || name.ends_with(".pem")
            || name.ends_with(".key")
            || name.ends_with(".p12")
            || name.ends_with(".pfx")
    })
}
