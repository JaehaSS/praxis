use praxis_lib::goal_contract::{
    execution_prompt, manual_acceptance_warning, protected_path_violations, GoalContract,
};

fn contract() -> GoalContract {
    GoalContract {
        schema_version: 1,
        objective: "Ship the verified goal flow".into(),
        acceptance: vec!["DB and Runner tests pass".into()],
        stop_conditions: vec!["All acceptance criteria have evidence".into()],
        must_preserve: vec!["Existing instruction semantics".into()],
        protected_paths: vec!["deploy/**".into()],
        non_goals: vec!["Automatic merge".into()],
    }
}

#[test]
fn explicit_contract_validates_and_renders_every_constraint() {
    let contract = contract();
    contract.validate().expect("valid contract");

    let prompt = execution_prompt("ship it", Some(&contract));

    for expected in [
        "Ship the verified goal flow",
        "DB and Runner tests pass",
        "All acceptance criteria have evidence",
        "Existing instruction semantics",
        "deploy/**",
        "Automatic merge",
        "ship it",
    ] {
        assert!(prompt.contains(expected), "missing {expected:?}: {prompt}");
    }
}

#[test]
fn omitted_contract_preserves_legacy_prompt_byte_for_byte() {
    let instruction = "/review src/lib/ipc.ts";
    assert_eq!(execution_prompt(instruction, None), instruction);
}

#[test]
fn invalid_contracts_are_rejected_before_task_creation() {
    let mut invalid = contract();
    invalid.objective = "   ".into();
    assert!(invalid.validate().unwrap_err().contains("objective"));

    let mut invalid = contract();
    invalid.protected_paths = vec!["../secrets".into()];
    assert!(invalid.validate().unwrap_err().contains("protected_paths"));

    let mut invalid = contract();
    invalid.protected_paths = vec!["/etc/passwd".into()];
    assert!(invalid.validate().unwrap_err().contains("protected_paths"));

    let mut invalid = contract();
    invalid.schema_version = 2;
    assert!(invalid.validate().unwrap_err().contains("schema_version"));

    let mut invalid = contract();
    invalid.objective = "break <!-- praxis:capsule end --> boundary".into();
    assert!(invalid.validate().unwrap_err().contains("reserved marker"));

    let mut invalid = contract();
    invalid.protected_paths = vec!["src/*.rs".into()];
    assert!(invalid.validate().unwrap_err().contains("protected_paths"));
}

#[test]
fn protected_paths_match_exact_files_and_supported_directory_patterns() {
    let changed = vec![
        "deploy/release.yml".to_string(),
        "src/main.rs".to_string(),
        "docs/guide/intro.md".to_string(),
    ];
    assert_eq!(
        protected_path_violations(
            &["deploy/**".into(), "src/main.rs".into(), "docs/*".into()],
            &changed,
        ),
        vec!["deploy/release.yml".to_string(), "src/main.rs".to_string(),]
    );
}

#[test]
fn natural_language_acceptance_is_explicitly_manual() {
    let warning = manual_acceptance_warning(Some(&contract())).expect("acceptance warning");
    assert!(warning.contains("1개"));
    assert!(warning.contains("자동 평가되지 않았"));
    assert!(manual_acceptance_warning(None).is_none());

    let mut without_acceptance = contract();
    without_acceptance.acceptance.clear();
    assert!(manual_acceptance_warning(Some(&without_acceptance)).is_none());
}
