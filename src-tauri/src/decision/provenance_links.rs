use super::provenance::{validate_hex, ApprovalProvenance, ArtifactKind, ArtifactLink, Relation};

pub(super) fn links(provenance: &ApprovalProvenance) -> anyhow::Result<Vec<ArtifactLink>> {
    provenance.decision_key_hash()?;
    let mut links = fixed_links(provenance);
    append_numeric(
        &mut links,
        &provenance.start_receipts,
        ArtifactKind::TaskStartReceipt,
    )?;
    append_memory_versions(&mut links, &provenance.memory_versions)?;
    append_numeric(
        &mut links,
        &provenance.evidence_checks,
        ArtifactKind::EvidenceCheck,
    )?;
    append_verification(&mut links, provenance.verification_run.as_deref())?;
    sort_and_deduplicate(&mut links);
    Ok(links)
}

fn fixed_links(provenance: &ApprovalProvenance) -> Vec<ArtifactLink> {
    vec![
        link(
            Relation::ApprovedBy,
            ArtifactKind::Actor,
            "local-human".into(),
        ),
        link(
            Relation::DerivedFrom,
            ArtifactKind::Task,
            provenance.task_id.to_string(),
        ),
        link(
            Relation::Used,
            ArtifactKind::InstructionDigest,
            provenance.instruction_digest.clone(),
        ),
        link(
            Relation::Generated,
            ArtifactKind::GitCommit,
            provenance.commit_sha.clone(),
        ),
    ]
}

fn append_numeric(
    links: &mut Vec<ArtifactLink>,
    values: &[i64],
    kind: ArtifactKind,
) -> anyhow::Result<()> {
    for value in values {
        if *value <= 0 {
            anyhow::bail!("artifact identity must be positive");
        }
        links.push(link(Relation::Used, kind, value.to_string()));
    }
    Ok(())
}

fn append_memory_versions(
    links: &mut Vec<ArtifactLink>,
    versions: &[(i64, i64)],
) -> anyhow::Result<()> {
    for (memory_id, version) in versions {
        if *memory_id <= 0 || *version <= 0 {
            anyhow::bail!("memory version identity must be positive");
        }
        links.push(link(
            Relation::Used,
            ArtifactKind::MemoryVersion,
            format!("{memory_id}:{version}"),
        ));
    }
    Ok(())
}

fn append_verification(
    links: &mut Vec<ArtifactLink>,
    reference: Option<&str>,
) -> anyhow::Result<()> {
    let Some(reference) = reference else {
        return Ok(());
    };
    validate_hex("verification run", reference, 64, 64)?;
    links.push(link(
        Relation::Used,
        ArtifactKind::VerificationRun,
        reference.into(),
    ));
    Ok(())
}

fn sort_and_deduplicate(links: &mut Vec<ArtifactLink>) {
    links.sort_by(|left, right| {
        (
            left.kind.as_str(),
            &left.artifact_ref,
            left.relation.as_str(),
        )
            .cmp(&(
                right.kind.as_str(),
                &right.artifact_ref,
                right.relation.as_str(),
            ))
    });
    links.dedup();
}

fn link(relation: Relation, kind: ArtifactKind, artifact_ref: String) -> ArtifactLink {
    ArtifactLink {
        relation,
        kind,
        artifact_ref,
    }
}
