//! Immutable task goal contract shared by local and Runner execution paths.

use serde::{Deserialize, Serialize};

mod path_policy;

pub use path_policy::protected_path_violations;
// 계약 필드를 사전 생성하는 crate 내부 경로가 동일한 검증 규칙을 재사용하도록 재노출 (중복 구현 금지).
pub(crate) use path_policy::valid_protected_pattern;

pub const SCHEMA_VERSION: u8 = 1;
const MAX_OBJECTIVE_BYTES: usize = 12_000;
// 계약 필드 상한은 crate 내부 공유 불변식 — 소비처에서 값 복사 금지(drift 방지).
pub(crate) const MAX_ITEMS: usize = 32;
pub(crate) const MAX_ITEM_BYTES: usize = 2_000;
const MAX_CONTRACT_BYTES: usize = 64_000;
const RESERVED_MARKERS: [&str; 2] = [
    "<!-- praxis:capsule begin -->",
    "<!-- praxis:capsule end -->",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalContract {
    pub schema_version: u8,
    pub objective: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub stop_conditions: Vec<String>,
    #[serde(default)]
    pub must_preserve: Vec<String>,
    #[serde(default)]
    pub protected_paths: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
}

impl GoalContract {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "지원하지 않는 goal contract schema_version: {}",
                self.schema_version
            ));
        }
        if self.objective.trim().is_empty() || self.objective.len() > MAX_OBJECTIVE_BYTES {
            return Err(format!(
                "goal contract objective는 1..={MAX_OBJECTIVE_BYTES} bytes여야 합니다"
            ));
        }
        reject_reserved_markers("objective", &self.objective)?;
        for (name, items) in [
            ("acceptance", &self.acceptance),
            ("stop_conditions", &self.stop_conditions),
            ("must_preserve", &self.must_preserve),
            ("non_goals", &self.non_goals),
        ] {
            validate_items(name, items)?;
        }
        validate_items("protected_paths", &self.protected_paths)?;
        if self
            .protected_paths
            .iter()
            .any(|path| !path_policy::valid_protected_pattern(path))
        {
            return Err(
                "goal contract protected_paths는 traversal 없는 repo-relative '/' 패턴이어야 합니다"
                    .into(),
            );
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("goal contract 직렬화 실패: {error}"))?
            .len();
        if bytes > MAX_CONTRACT_BYTES {
            return Err(format!(
                "goal contract는 최대 {MAX_CONTRACT_BYTES} bytes까지 허용됩니다"
            ));
        }
        Ok(())
    }
}

fn validate_items(name: &str, items: &[String]) -> Result<(), String> {
    if items.len() > MAX_ITEMS {
        return Err(format!("goal contract {name}는 최대 {MAX_ITEMS}개입니다"));
    }
    if items
        .iter()
        .any(|item| item.trim().is_empty() || item.len() > MAX_ITEM_BYTES)
    {
        return Err(format!(
            "goal contract {name} 항목은 1..={MAX_ITEM_BYTES} bytes여야 합니다"
        ));
    }
    for item in items {
        reject_reserved_markers(name, item)?;
    }
    Ok(())
}

fn reject_reserved_markers(field: &str, value: &str) -> Result<(), String> {
    if RESERVED_MARKERS.iter().any(|marker| value.contains(marker)) {
        return Err(format!(
            "goal contract {field}에 reserved marker를 사용할 수 없습니다"
        ));
    }
    Ok(())
}

/// Preserve the legacy prompt exactly when no explicit contract was supplied.
pub fn execution_prompt(instruction: &str, contract: Option<&GoalContract>) -> String {
    let Some(contract) = contract else {
        return instruction.to_string();
    };
    let mut prompt = format!(
        "# Praxis Goal Contract (v{})\n\n## Objective\n{}\n",
        contract.schema_version, contract.objective
    );
    push_section(
        &mut prompt,
        "Acceptance (manual unless deterministic evidence exists)",
        &contract.acceptance,
    );
    push_section(
        &mut prompt,
        "Advisory stop conditions (agent must pause and ask)",
        &contract.stop_conditions,
    );
    push_section(&mut prompt, "Must preserve", &contract.must_preserve);
    push_section(
        &mut prompt,
        "Protected paths (approval is blocked if changed)",
        &contract.protected_paths,
    );
    push_section(&mut prompt, "Non-goals", &contract.non_goals);
    if instruction.trim() != contract.objective.trim() {
        prompt.push_str("\n## Original task request\n");
        prompt.push_str(instruction);
        prompt.push('\n');
    }
    prompt
}

/// Natural-language acceptance is never promoted to mechanical evidence in v1.
pub fn manual_acceptance_warning(contract: Option<&GoalContract>) -> Option<String> {
    let count = contract?.acceptance.len();
    (count > 0).then(|| {
        format!(
            "Goal Contract acceptance {count}개는 자동 평가되지 않았습니다 — 사람이 결과와 증거를 확인해야 합니다"
        )
    })
}

fn push_section(output: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    output.push_str("\n## ");
    output.push_str(title);
    output.push('\n');
    for item in items {
        output.push_str("- ");
        output.push_str(item);
        output.push('\n');
    }
}
