//! Deterministic, conservative composition of validated ancestor deltas.

use std::collections::BTreeMap;

use anyhow::{bail, Result};

use super::artifacts::{Delta, EntryKind, EntryState, TreeManifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncestorDelta {
    pub node_id: String,
    pub topo_order: u32,
    pub delta: Delta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub manifest: TreeManifest,
    pub applied_ancestors: Vec<String>,
}

/// Hash-only callers conservatively reject concurrent content changes.
pub fn merge_ancestors(base: &TreeManifest, ancestors: Vec<AncestorDelta>) -> Result<MergeResult> {
    merge_with_content(base, ancestors, |path, _, _, _| {
        bail!("input_conflict at {path}: content merger required")
    })
}

/// The artifact store supplies a merger that verifies the exact before/after
/// blobs. Keeping it here preserves identical delete/type/mode semantics.
pub(super) fn merge_with_content(
    base: &TreeManifest,
    ancestors: Vec<AncestorDelta>,
    mut merge: impl FnMut(&str, &EntryState, &EntryState, &EntryState) -> Result<EntryState>,
) -> Result<MergeResult> {
    base.validate()?;
    let mut by_id = BTreeMap::new();
    for ancestor in ancestors {
        if ancestor.node_id.is_empty() {
            bail!("ancestor node id must not be empty");
        }
        ancestor.delta.validate()?;
        if let Some(existing) = by_id.insert(ancestor.node_id.clone(), ancestor.clone()) {
            if existing.delta != ancestor.delta || existing.topo_order != ancestor.topo_order {
                bail!(
                    "ancestor supplied more than once with different content: {}",
                    ancestor.node_id
                );
            }
        }
    }
    let mut ancestors = by_id.into_values().collect::<Vec<_>>();
    ancestors.sort_by(|left, right| {
        left.topo_order
            .cmp(&right.topo_order)
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    let mut candidate = base.states();
    let mut applied_ancestors = Vec::new();
    for ancestor in ancestors {
        let mut next = candidate.clone();
        for entry in &ancestor.delta.entries {
            apply_entry(
                &mut next,
                entry.path.as_str(),
                entry.before.as_ref(),
                entry.after.as_ref(),
                &mut merge,
            )?;
        }
        TreeManifest::from_states(next.clone())
            .map_err(|error| anyhow::anyhow!("input_conflict: {error}"))?;
        candidate = next;
        applied_ancestors.push(ancestor.node_id);
    }
    Ok(MergeResult {
        manifest: TreeManifest::from_states(candidate)?,
        applied_ancestors,
    })
}

fn apply_entry(
    candidate: &mut BTreeMap<String, EntryState>,
    path: &str,
    base: Option<&EntryState>,
    theirs: Option<&EntryState>,
    merge: &mut impl FnMut(&str, &EntryState, &EntryState, &EntryState) -> Result<EntryState>,
) -> Result<()> {
    let ours = candidate.get(path).cloned();
    if ours.as_ref() == base {
        set_state(candidate, path, theirs.cloned());
        return Ok(());
    }
    if theirs == base || ours.as_ref() == theirs {
        return Ok(());
    }

    // A file's content and executable bit can be independently changed.
    if let (Some(base), Some(ours), Some(theirs)) = (base, ours.as_ref(), theirs) {
        if base.kind == EntryKind::File
            && ours.kind == EntryKind::File
            && theirs.kind == EntryKind::File
        {
            let mode = if ours.mode == base.mode {
                theirs.mode
            } else if theirs.mode == base.mode || ours.mode == theirs.mode {
                ours.mode
            } else {
                bail!("input_conflict at {path}: mode conflict")
            };
            let mut combined = if ours.hash == base.hash {
                theirs.clone()
            } else if theirs.hash == base.hash || ours.hash == theirs.hash {
                ours.clone()
            } else {
                merge(path, base, ours, theirs)?
            };
            combined.mode = mode;
            set_state(candidate, path, Some(combined));
            return Ok(());
        }
    }
    bail!("input_conflict at {path}")
}

fn set_state(candidate: &mut BTreeMap<String, EntryState>, path: &str, state: Option<EntryState>) {
    match state {
        Some(state) => {
            candidate.insert(path.to_owned(), state);
        }
        None => {
            candidate.remove(path);
        }
    }
}
