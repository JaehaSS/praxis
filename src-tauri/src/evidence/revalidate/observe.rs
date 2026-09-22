use std::path::Path;

use super::LoadedEvidence;
use crate::evidence::model::{
    CodeLocator, EvidenceRecord, ExternalDocumentLocator, LocalDocumentLocator, Observation,
};
use crate::memory::{evidence_kind, evidence_status};

pub(super) fn all(loaded: &LoadedEvidence, now: i64) -> Vec<Observation> {
    loaded
        .evidence
        .iter()
        .cloned()
        .map(|evidence| one(evidence, loaded.scope_key.as_deref(), now))
        .collect()
}

fn one(evidence: EvidenceRecord, scope: Option<&str>, now: i64) -> Observation {
    if evidence.expires_at.is_some_and(|expiry| expiry <= now) {
        return result(evidence, evidence_status::EXPIRED, None);
    }
    match evidence.kind.as_str() {
        evidence_kind::USER_CONFIRMATION => result(evidence, evidence_status::VALID, None),
        evidence_kind::CODE_LOCATION => code(evidence, scope),
        evidence_kind::DOCUMENT => document(evidence, scope),
        _ => result(evidence, evidence_status::UNKNOWN, None),
    }
}

fn code(evidence: EvidenceRecord, scope: Option<&str>) -> Observation {
    let Ok(locator) = serde_json::from_str::<CodeLocator>(&evidence.locator_json) else {
        return result(evidence, evidence_status::UNKNOWN, None);
    };
    if locator.schema_version != 1 {
        return result(evidence, evidence_status::UNKNOWN, None);
    }
    let Some(scope) = scope else {
        return result(evidence, evidence_status::UNKNOWN, None);
    };
    let snapshot = crate::evidence::source_snapshot::code(
        Path::new(scope),
        &locator.relative_path,
        locator.line_start,
        locator.line_end,
    );
    match snapshot {
        Ok(value)
            if same_repository(scope, &locator.canonical_repository)
                && Some(value.hash.as_str()) == evidence.snapshot_hash.as_deref()
                && value.commit_oid == locator.commit_oid =>
        {
            result(evidence, evidence_status::VALID, Some(value.hash))
        }
        Ok(value) => result(evidence, evidence_status::CHANGED, Some(value.hash)),
        Err(crate::evidence::source_snapshot::ObserveError::Missing) => {
            result(evidence, evidence_status::MISSING, None)
        }
        Err(_) => result(evidence, evidence_status::UNKNOWN, None),
    }
}

fn document(evidence: EvidenceRecord, scope: Option<&str>) -> Observation {
    if let Ok(locator) = serde_json::from_str::<ExternalDocumentLocator>(&evidence.locator_json) {
        let canonical = crate::evidence::source_snapshot::external_url(&locator.url);
        if locator.schema_version == 1
            && evidence.expires_at.is_some()
            && canonical.is_ok_and(|url| url == locator.url)
        {
            return result(evidence, evidence_status::VALID, None);
        }
        return result(evidence, evidence_status::UNKNOWN, None);
    }
    let Ok(locator) = serde_json::from_str::<LocalDocumentLocator>(&evidence.locator_json) else {
        return result(evidence, evidence_status::UNKNOWN, None);
    };
    if locator.schema_version != 1 {
        return result(evidence, evidence_status::UNKNOWN, None);
    }
    let Some(scope) = scope else {
        return result(evidence, evidence_status::UNKNOWN, None);
    };
    match crate::evidence::source_snapshot::document(Path::new(scope), &locator.relative_path) {
        Ok(hash)
            if same_repository(scope, &locator.canonical_repository)
                && Some(hash.as_str()) == evidence.snapshot_hash.as_deref() =>
        {
            result(evidence, evidence_status::VALID, Some(hash))
        }
        Ok(hash) => result(evidence, evidence_status::CHANGED, Some(hash)),
        Err(crate::evidence::source_snapshot::ObserveError::Missing) => {
            result(evidence, evidence_status::MISSING, None)
        }
        Err(_) => result(evidence, evidence_status::UNKNOWN, None),
    }
}

fn same_repository(scope: &str, expected: &str) -> bool {
    Path::new(scope)
        .canonicalize()
        .is_ok_and(|path| path == Path::new(expected))
}

fn result(evidence: EvidenceRecord, status: &str, hash: Option<String>) -> Observation {
    Observation {
        evidence,
        status: status.to_string(),
        observed_hash: hash,
    }
}
