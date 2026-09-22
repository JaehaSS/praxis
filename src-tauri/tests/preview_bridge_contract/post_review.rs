#[test]
fn payload_constructor_uses_json_text_and_rejects_invalid_json() {
    let model = include_str!("../../src/preview_bridge/model.rs");
    let registry = include_str!("../../src/preview_bridge/registry.rs");

    assert!(model.contains("pub body: String"));
    assert!(model.contains("InvalidJson"));
    assert!(registry.contains("serde_json::from_str"));
}

#[test]
fn supplied_sha_is_preserved_by_constructor_contract() {
    let model = include_str!("../../src/preview_bridge/model.rs");

    assert!(model.contains("sha256: sha256.into()"));
    assert!(!model.contains("sha256_hex(&body)"));
}

#[test]
fn probe_command_identity_is_native_random_and_never_a_fixed_literal() {
    let model = include_str!("../../src/preview_bridge/model.rs");
    let commands = concat!(
        include_str!("../../src/commands.rs"),
        // designmode/preview 커맨드는 commands/designmode.rs로 갈라졌다.
        include_str!("../../src/commands/designmode.rs"),
    );

    assert!(model.contains("pub fn new_command_id"));
    assert!(model.contains("[0_u8; 16]"));
    assert!(!commands.contains("strict-csp-probe"));
}

#[test]
fn probe_evidence_is_bound_to_its_task_label_and_command_id() {
    let runner = include_str!("packaged_macos_probe.mjs");

    for field in ["taskId", "commandId", "-4242", "designmode-probe"] {
        assert!(runner.contains(field), "missing probe binding: {field}");
    }
    assert!(runner.contains("/^[a-f0-9]{32}$/"));
}

#[test]
fn probe_url_validation_is_loopback_http_with_an_explicit_port_only() {
    let commands = concat!(
        include_str!("../../src/commands.rs"),
        // designmode/preview 커맨드는 commands/designmode.rs로 갈라졌다.
        include_str!("../../src/commands/designmode.rs"),
    );

    assert!(commands.contains("validate_preview_probe_url"));
    for denied in ["https", "username", "port()", "localhost", "127.0.0.1"] {
        assert!(commands.contains(denied));
    }
}

#[test]
fn navigation_prepares_generation_before_native_navigate_and_stays_cancelled_on_error() {
    let commands = concat!(
        include_str!("../../src/commands.rs"),
        // designmode/preview 커맨드는 commands/designmode.rs로 갈라졌다.
        include_str!("../../src/commands/designmode.rs"),
    );
    let bridge = include_str!("../../src/preview_bridge/registry.rs");

    assert!(commands.contains("prepare_navigation"));
    assert!(commands.contains("NAVIGATION_CANCELLED"));
    assert!(bridge.contains("prepare_navigation"));
    assert!(bridge.contains("observe_navigation"));
}

#[test]
fn registry_module_remains_within_the_repository_file_limit() {
    assert!(
        include_str!("../../src/preview_bridge/registry.rs")
            .lines()
            .count()
            <= 200
    );
}
