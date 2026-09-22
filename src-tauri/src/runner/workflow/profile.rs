//! Versioned, fail-closed configuration for the Linux workflow sandbox.
//!
//! These types are deliberately small enough to deserialize from the Runner
//! configuration.  They contain references and paths, never credential values.

use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const RUNTIME_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const FORWARDER_CONTRACT_V1: &str = "praxis-egress-forwarder-v1";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeProfile {
    pub schema_version: u32,
    pub id: String,
    /// An image reference which includes its immutable sha256 manifest digest.
    pub image: String,
    /// The Podman binary installed by the Linux runner.  An absolute path avoids
    /// using a user-controlled PATH during sandbox construction.
    pub podman_executable: PathBuf,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub pids_limit: u32,
    pub timeouts: RuntimeTimeouts,
    #[serde(default)]
    pub vendor: Option<VendorProfile>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTimeouts {
    pub probe_ms: u64,
    pub create_ms: u64,
    pub start_ms: u64,
    pub inspect_ms: u64,
    pub stop_ms: u64,
    pub remove_ms: u64,
    pub logs_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VendorProfile {
    pub id: String,
    /// Executable supplied by the prepared image. It must run the fixed
    /// forwarder contract below; arbitrary shell snippets are not accepted.
    pub executable: PathBuf,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub tls_domains: Vec<String>,
    /// Logical secret identifier only. The actual per-attempt temporary file is
    /// supplied separately and is never serialized into a receipt or a log.
    pub credential_file_reference: Option<String>,
    pub forwarder_path: PathBuf,
    pub forwarder_contract: String,
}

/// A server-registered command, selected by ID from a workflow spec.  Worker
/// text can never fill `executable`, `argv`, or environment keys.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegisteredCommandProfile {
    pub id: String,
    pub runtime_profile_id: String,
    pub vendor_id: Option<String>,
    pub executable: PathBuf,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl RuntimeProfile {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == RUNTIME_PROFILE_SCHEMA_VERSION,
            "unsupported runtime profile schema version"
        );
        validate_id("runtime profile", &self.id)?;
        ensure!(
            is_pinned_image(&self.image),
            "runtime profile image must contain an exact @sha256 digest"
        );
        ensure!(
            self.podman_executable.is_absolute(),
            "podman executable must be an absolute path"
        );
        validate_resource(&self.cpu_limit, "cpu_limit")?;
        validate_resource(&self.memory_limit, "memory_limit")?;
        ensure!(self.pids_limit > 0, "pids_limit must be positive");
        self.timeouts.validate()?;
        if let Some(vendor) = &self.vendor {
            vendor.validate()?;
        }
        Ok(())
    }

    pub fn image_digest(&self) -> &str {
        self.image
            .rsplit_once('@')
            .map(|(_, digest)| digest)
            .unwrap_or("")
    }
}

impl RuntimeTimeouts {
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("probe_ms", self.probe_ms),
            ("create_ms", self.create_ms),
            ("start_ms", self.start_ms),
            ("inspect_ms", self.inspect_ms),
            ("stop_ms", self.stop_ms),
            ("remove_ms", self.remove_ms),
            ("logs_ms", self.logs_ms),
        ] {
            ensure!(
                (1..=30_000).contains(&value),
                "{name} must be between 1 and 30000 ms"
            );
        }
        Ok(())
    }
}

impl VendorProfile {
    pub fn validate(&self) -> Result<()> {
        validate_id("vendor", &self.id)?;
        ensure!(
            self.executable.is_absolute(),
            "vendor executable must be an absolute container path"
        );
        ensure!(
            self.forwarder_path.is_absolute(),
            "vendor forwarder_path must be an absolute container path"
        );
        ensure!(
            self.forwarder_contract == FORWARDER_CONTRACT_V1,
            "vendor does not declare the supported egress forwarder contract"
        );
        ensure!(
            !self.tls_domains.is_empty(),
            "vendor TLS allowlist must not be empty"
        );
        let mut domains = BTreeSet::new();
        for domain in &self.tls_domains {
            let normalized = validate_tls_domain(domain)?;
            ensure!(
                domains.insert(normalized),
                "vendor TLS allowlist contains a duplicate domain"
            );
        }
        validate_env(&self.env)
    }
}

impl RegisteredCommandProfile {
    pub fn validate_for(&self, runtime: &RuntimeProfile) -> Result<()> {
        validate_id("command", &self.id)?;
        ensure!(
            self.runtime_profile_id == runtime.id,
            "command profile is registered for another runtime profile"
        );
        ensure!(
            self.executable.is_absolute(),
            "command executable must be an absolute container path"
        );
        validate_env(&self.env)?;
        match (&runtime.vendor, &self.vendor_id) {
            // Mechanical checks never receive a socket or credential mount.
            (_, None) => Ok(()),
            (Some(vendor), Some(id)) if id == &vendor.id => Ok(()),
            (Some(_), Some(_)) => bail!("command profile selects an unsupported vendor"),
            (None, Some(_)) => bail!("command profile selects an unsupported vendor"),
        }
    }
}

pub fn validate_tls_domain(value: &str) -> Result<String> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    ensure!(
        !value.is_empty() && value.len() <= 253,
        "TLS domain is empty or too long"
    );
    ensure!(
        value.parse::<std::net::IpAddr>().is_err(),
        "TLS allowlist entries must be DNS names"
    );
    for label in value.split('.') {
        ensure!(
            !label.is_empty() && label.len() <= 63,
            "TLS domain has an invalid label"
        );
        ensure!(
            !label.starts_with('-') && !label.ends_with('-'),
            "TLS domain label cannot start or end with '-'"
        );
        ensure!(
            label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "TLS domain contains an invalid character"
        );
    }
    ensure!(
        value.contains('.'),
        "TLS domain must contain a registrable suffix"
    );
    Ok(value)
}

fn validate_id(kind: &str, value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 128,
        "{kind} id is empty or too long"
    );
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "{kind} id contains an invalid character"
    );
    Ok(())
}

fn validate_resource(value: &str, name: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 64,
        "{name} is empty or too long"
    );
    if name == "cpu_limit" {
        ensure!(
            value
                .parse::<f64>()
                .is_ok_and(|value| value.is_finite() && value > 0.0),
            "cpu_limit must be a positive decimal"
        );
    } else {
        let digits = value.bytes().take_while(u8::is_ascii_digit).count();
        let (quantity, suffix) = value.split_at(digits);
        ensure!(
            !quantity.is_empty() && quantity.parse::<u64>().is_ok_and(|value| value > 0),
            "memory_limit must start with a positive integer"
        );
        ensure!(
            matches!(
                suffix.to_ascii_lowercase().as_str(),
                "" | "b"
                    | "k"
                    | "kb"
                    | "kib"
                    | "m"
                    | "mb"
                    | "mib"
                    | "g"
                    | "gb"
                    | "gib"
                    | "t"
                    | "tb"
                    | "tib"
            ),
            "memory_limit has an unsupported unit"
        );
    }
    Ok(())
}

fn validate_env(env: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in env {
        ensure!(
            !key.is_empty()
                && key.len() <= 128
                && key.bytes().enumerate().all(|(index, byte)| byte == b'_'
                    || byte.is_ascii_alphabetic()
                    || (index > 0 && byte.is_ascii_digit())),
            "environment key is invalid"
        );
        ensure!(
            value.len() <= 16 * 1024 && !value.contains('\0'),
            "environment value is invalid"
        );
        ensure!(
            !matches!(
                key.as_str(),
                "HOME"
                    | "TMPDIR"
                    | "TMP"
                    | "TEMP"
                    | "HTTP_PROXY"
                    | "http_proxy"
                    | "HTTPS_PROXY"
                    | "https_proxy"
                    | "ALL_PROXY"
                    | "all_proxy"
                    | "NO_PROXY"
                    | "no_proxy"
                    | "PRAXIS_WORKFLOW_PROMPT_FILE"
                    | "PRAXIS_WORKFLOW_CREDENTIAL_FILE"
            ),
            "environment key is controlled by the workflow sandbox"
        );
    }
    Ok(())
}

fn is_pinned_image(value: &str) -> bool {
    value.rsplit_once('@').is_some_and(|(reference, digest)| {
        !reference.is_empty()
            && digest.len() == 71
            && digest.starts_with("sha256:")
            && digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub fn path_is_safe_mount_source(path: &Path) -> bool {
    path.is_absolute() && path != Path::new("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unpinned_images_and_incomplete_forwarder_contracts() {
        let profile = RuntimeProfile {
            schema_version: 1,
            id: "linux".into(),
            image: "registry/worker:latest".into(),
            podman_executable: "/usr/bin/podman".into(),
            cpu_limit: "1".into(),
            memory_limit: "512m".into(),
            pids_limit: 64,
            timeouts: RuntimeTimeouts {
                probe_ms: 1,
                create_ms: 1,
                start_ms: 1,
                inspect_ms: 1,
                stop_ms: 1,
                remove_ms: 1,
                logs_ms: 1,
            },
            vendor: None,
        };
        assert!(profile.validate().is_err());
        assert!(validate_tls_domain("127.0.0.1").is_err());
        assert!(validate_tls_domain("api.vendor.example").is_ok());
    }
}
