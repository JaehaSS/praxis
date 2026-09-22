#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::runner::config::RunnerConfig;

#[test]
fn accepts_loopback_config_with_bounded_concurrency() {
    let config = RunnerConfig::from_toml(
        "bind = \"127.0.0.1:47831\"\nrepository_roots = [\"/tmp\"]\nmax_concurrent_tasks = 2\npairing_token_file = \"/tmp/praxis-token\"\n",
    )
    .unwrap();

    assert_eq!(config.max_concurrent_tasks, 2);
}

#[test]
fn canonicalizes_existing_repository_roots_and_rejects_missing_ones() {
    let root = temp_root::dir();
    let config = RunnerConfig::from_toml(&format!(
        "repository_roots = [\"{}\"]\npairing_token_file = \"/tmp/praxis-token\"",
        root.to_string_lossy()
    ))
    .unwrap();

    assert_eq!(
        config.repository_roots,
        vec![std::fs::canonicalize(&root).unwrap()]
    );
    assert!(
        RunnerConfig::from_toml("repository_roots = [\"/definitely/missing/praxis-root\"]\npairing_token_file = \"/tmp/praxis-token\"")
            .is_err()
    );
}

#[test]
fn rejects_non_loopback_and_invalid_concurrency() {
    assert!(
        RunnerConfig::from_toml("bind = \"0.0.0.0:47831\"\nrepository_roots = [\"/tmp\"]\npairing_token_file = \"/tmp/praxis-token\"").is_err()
    );
    assert!(RunnerConfig::from_toml(
        "bind = \"127.0.0.1:47831\"\nrepository_roots = [\"/tmp\"]\nmax_concurrent_tasks = 11\npairing_token_file = \"/tmp/praxis-token\""
    )
    .is_err());
}
