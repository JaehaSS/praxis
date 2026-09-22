use std::collections::BTreeSet;

use praxis_lib::workflow::WorkflowSpec;

const FIXTURE: &str = include_str!("fixtures/workflow-spec-v1.json");

fn fixture_value() -> serde_json::Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn parse(value: serde_json::Value) -> Result<WorkflowSpec, String> {
    WorkflowSpec::parse_json(&serde_json::to_string(&value).unwrap())
}

#[test]
fn fixture_roundtrips_and_has_deterministic_graph_queries() {
    let spec = WorkflowSpec::parse_json(FIXTURE).unwrap();
    let graph = spec.validate().unwrap();
    assert_eq!(
        graph.topological_order(),
        &[
            "api".to_owned(),
            "ui".to_owned(),
            "final-integration".to_owned()
        ]
    );
    assert_eq!(
        graph.ancestors_of("final-integration"),
        BTreeSet::from(["api".to_owned(), "ui".to_owned()])
    );
    assert_eq!(
        graph.descendants_of("api"),
        BTreeSet::from(["final-integration".to_owned()])
    );
    assert_eq!(
        spec.digest().unwrap(),
        WorkflowSpec::parse_json(&serde_json::to_string(&spec).unwrap())
            .unwrap()
            .digest()
            .unwrap()
    );
    assert_eq!(spec.task_execution_hash("api").unwrap().len(), 64);
}

#[test]
fn rejects_cycles_dangling_edges_and_tasks_outside_final_ancestry() {
    let mut cycle = fixture_value();
    cycle["edges"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "from": "final-integration", "to": "api" }));
    cycle["limits"]["max_edges"] = serde_json::json!(3);
    assert!(parse(cycle).unwrap_err().contains("cycle"));

    let mut dangling = fixture_value();
    dangling["edges"][0]["from"] = serde_json::json!("missing");
    assert!(parse(dangling).unwrap_err().contains("unknown task"));

    let mut disconnected = fixture_value();
    let task = disconnected["tasks"][0].clone();
    disconnected["tasks"].as_array_mut().unwrap().push(task);
    disconnected["tasks"][3]["id"] = serde_json::json!("orphan");
    disconnected["limits"]["max_tasks"] = serde_json::json!(4);
    assert!(parse(disconnected)
        .unwrap_err()
        .contains("must reach final_task_id"));
}

#[test]
fn rejects_unknown_nested_fields_and_unsafe_paths() {
    let mut unknown = fixture_value();
    unknown["tasks"][0]["retry_policy"]["surprise"] = serde_json::json!(true);
    assert!(parse(unknown).unwrap_err().contains("unknown field"));

    for unsafe_path in [
        "../outside",
        "src/**",
        "/absolute",
        "src\\windows",
        "src//double",
        "src/./current",
    ] {
        let mut value = fixture_value();
        value["tasks"][0]["write_paths"][0] = serde_json::json!(unsafe_path);
        assert!(parse(value).is_err(), "accepted unsafe path {unsafe_path}");
    }
}

#[test]
fn parse_enforces_byte_and_item_limits_and_exact_git_sha() {
    let oversized = format!("{}{}", FIXTURE, " ".repeat(256 * 1024));
    assert!(WorkflowSpec::parse_json(&oversized)
        .unwrap_err()
        .contains("exceeds"));

    let mut too_many_tasks = fixture_value();
    too_many_tasks["limits"]["max_tasks"] = serde_json::json!(2);
    assert!(parse(too_many_tasks)
        .unwrap_err()
        .contains("limits.max_tasks"));

    let mut abbreviated_sha = fixture_value();
    abbreviated_sha["base_commit"] = serde_json::json!("0123456");
    assert!(parse(abbreviated_sha).unwrap_err().contains("Git SHA"));
}

#[test]
fn diamond_graph_uses_stable_topology_and_complete_ancestors() {
    let mut diamond = fixture_value();
    let mut prepare = diamond["tasks"][0].clone();
    prepare["id"] = serde_json::json!("prepare");
    prepare["objective"] = serde_json::json!("Prepare shared input.");
    prepare["write_paths"] = serde_json::json!(["src/prepare"]);
    prepare["output_contract"] =
        serde_json::json!({ "include_paths": ["src/prepare"], "exclude_paths": [] });
    prepare["resource_requests_by_step"] = serde_json::json!({});
    prepare["checks"] = serde_json::json!([]);
    prepare["manual_acceptance"] = serde_json::json!([]);
    prepare["retry_policy"] =
        serde_json::json!({ "max_attempts": 1, "auto_retry_transient": false });
    diamond["tasks"].as_array_mut().unwrap().push(prepare);
    diamond["edges"] = serde_json::json!([
        { "from": "prepare", "to": "api" },
        { "from": "prepare", "to": "ui" },
        { "from": "api", "to": "final-integration" },
        { "from": "ui", "to": "final-integration" }
    ]);
    diamond["limits"] = serde_json::json!({
        "max_tasks": 4,
        "max_edges": 4,
        "max_attempts_per_task": 3,
        "max_concurrent_tasks": 2
    });
    let graph = parse(diamond).unwrap().validate().unwrap();
    assert_eq!(
        graph.topological_order(),
        &[
            "prepare".to_owned(),
            "api".to_owned(),
            "ui".to_owned(),
            "final-integration".to_owned()
        ]
    );
    assert_eq!(
        graph.ancestors_of("final-integration"),
        BTreeSet::from(["prepare".to_owned(), "api".to_owned(), "ui".to_owned()])
    );
}

#[test]
fn revision_impact_follows_new_and_removed_edge_targets_through_the_union() {
    let previous = WorkflowSpec::parse_json(FIXTURE).unwrap();
    let graph = previous.validate().unwrap();
    let mut revised = fixture_value();
    revised["edges"] = serde_json::json!([
        { "from": "api", "to": "ui" },
        { "from": "ui", "to": "final-integration" }
    ]);
    revised["tasks"][2]["input_artifacts"] = serde_json::json!([
        { "task_id": "ui", "artifact": "delta" }
    ]);
    let revised = parse(revised).unwrap();
    assert_eq!(
        graph.revision_impact(&previous, &revised).unwrap(),
        BTreeSet::from(["ui".to_owned(), "final-integration".to_owned()])
    );
}
