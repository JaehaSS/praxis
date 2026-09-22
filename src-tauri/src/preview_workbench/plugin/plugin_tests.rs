use super::*;

fn identity() -> ToolbarIdentity {
    ToolbarIdentity {
        app_epoch: "epoch".into(),
        task_id: 7,
        toolbar_label: "previewbar-7-2".into(),
        window_generation: 2,
    }
}

#[test]
fn blank_ready_adopts_the_actual_toolbar_identity() {
    let mut message = serde_json::json!({
        "kind": "ready", "appEpoch": "", "taskId": 0,
        "toolbarLabel": "", "windowGeneration": 0
    });
    canonicalize_ready(
        &mut message,
        ready_identity("previewbar-7-2", Some(identity())).unwrap(),
    )
    .unwrap();
    assert_eq!(message["taskId"], 7);
    assert_eq!(message["toolbarLabel"], "previewbar-7-2");
}

#[test]
fn ready_rejects_foreign_or_closed_toolbar_callers() {
    assert!(ready_identity("previewbar-8-2", Some(identity())).is_err());
    assert!(ready_identity("previewbar-7-2", None).is_err());
}
