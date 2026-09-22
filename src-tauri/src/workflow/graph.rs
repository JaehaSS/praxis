//! Deterministic graph operations for a validated workflow revision.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::model::WorkflowSpec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedGraph {
    topological_order: Vec<String>,
    ancestors: BTreeMap<String, BTreeSet<String>>,
    descendants: BTreeMap<String, BTreeSet<String>>,
    edges: BTreeSet<(String, String)>,
}

impl ValidatedGraph {
    pub(crate) fn build(spec: &WorkflowSpec) -> Result<Self, String> {
        let nodes: BTreeSet<_> = spec.tasks.iter().map(|task| task.id.clone()).collect();
        let edges: BTreeSet<_> = spec
            .edges
            .iter()
            .map(|edge| (edge.from.clone(), edge.to.clone()))
            .collect();
        let mut children: BTreeMap<String, BTreeSet<String>> = nodes
            .iter()
            .map(|node| (node.clone(), BTreeSet::new()))
            .collect();
        let mut parents = children.clone();
        for (from, to) in &edges {
            children
                .get_mut(from)
                .expect("validated edge source")
                .insert(to.clone());
            parents
                .get_mut(to)
                .expect("validated edge target")
                .insert(from.clone());
        }
        let mut indegree: BTreeMap<_, _> = parents
            .iter()
            .map(|(node, values)| (node.clone(), values.len()))
            .collect();
        let mut ready: BTreeSet<_> = indegree
            .iter()
            .filter_map(|(node, degree)| (*degree == 0).then_some(node.clone()))
            .collect();
        let mut topological_order = Vec::with_capacity(nodes.len());
        while let Some(node) = ready.pop_first() {
            topological_order.push(node.clone());
            for child in &children[&node] {
                let degree = indegree.get_mut(child).expect("child indegree");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(child.clone());
                }
            }
        }
        if topological_order.len() != nodes.len() {
            return Err("workflow graph contains a cycle".into());
        }
        let ancestors = transitive_sets(&nodes, &parents);
        let descendants = transitive_sets(&nodes, &children);
        Ok(Self {
            topological_order,
            ancestors,
            descendants,
            edges,
        })
    }

    pub fn topological_order(&self) -> &[String] {
        &self.topological_order
    }

    pub fn ancestors_of(&self, task_id: &str) -> BTreeSet<String> {
        self.ancestors.get(task_id).cloned().unwrap_or_default()
    }

    pub fn descendants_of(&self, task_id: &str) -> BTreeSet<String> {
        self.descendants.get(task_id).cloned().unwrap_or_default()
    }

    /// Returns all old/new nodes whose accepted result must be reconsidered for `next`.
    /// The union may be cyclic even though both revisions were separately valid, so this
    /// deliberately uses a visited traversal instead of a topological walk.
    pub fn revision_impact(
        &self,
        previous: &WorkflowSpec,
        next: &WorkflowSpec,
    ) -> Result<BTreeSet<String>, String> {
        next.validate()?;
        let mut direct = BTreeSet::new();
        if previous.project_ref != next.project_ref
            || previous.limits != next.limits
            || previous.base_commit != next.base_commit
            || previous.execution_profile_id != next.execution_profile_id
        {
            direct.extend(previous.tasks.iter().map(|task| task.id.clone()));
            direct.extend(next.tasks.iter().map(|task| task.id.clone()));
        } else {
            let old_ids: BTreeSet<_> = previous.tasks.iter().map(|task| task.id.as_str()).collect();
            let new_ids: BTreeSet<_> = next.tasks.iter().map(|task| task.id.as_str()).collect();
            direct.extend(
                old_ids
                    .symmetric_difference(&new_ids)
                    .map(|id| (*id).to_owned()),
            );
            for id in old_ids.intersection(&new_ids) {
                if previous.task_execution_hash(id)? != next.task_execution_hash(id)? {
                    direct.insert((*id).to_owned());
                }
            }
        }
        if previous.final_task_id != next.final_task_id {
            direct.insert(previous.final_task_id.clone());
            direct.insert(next.final_task_id.clone());
        }
        let next_edges: BTreeSet<_> = next
            .edges
            .iter()
            .map(|edge| (edge.from.clone(), edge.to.clone()))
            .collect();
        for (_, target) in self.edges.symmetric_difference(&next_edges) {
            direct.insert(target.clone());
        }
        let mut union_children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (from, to) in self.edges.iter().chain(next_edges.iter()) {
            union_children
                .entry(from.clone())
                .or_default()
                .insert(to.clone());
            union_children.entry(to.clone()).or_default();
        }
        let mut impacted = direct.clone();
        let mut pending: VecDeque<_> = direct.into_iter().collect();
        while let Some(node) = pending.pop_front() {
            for child in union_children.get(&node).into_iter().flatten() {
                if impacted.insert(child.clone()) {
                    pending.push_back(child.clone());
                }
            }
        }
        Ok(impacted)
    }
}

fn transitive_sets(
    nodes: &BTreeSet<String>,
    neighbours: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, BTreeSet<String>> {
    nodes
        .iter()
        .map(|node| {
            let mut reached = BTreeSet::new();
            let mut pending: VecDeque<_> = neighbours[node].iter().cloned().collect();
            while let Some(next) = pending.pop_front() {
                if reached.insert(next.clone()) {
                    pending.extend(neighbours[&next].iter().cloned());
                }
            }
            (node.clone(), reached)
        })
        .collect()
}
