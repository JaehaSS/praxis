use super::*;

#[test]
fn prepare_replays_one_correlation_after_rejection_or_retirement() {
    let workbench = PreviewWorkbench::with_epoch("epoch".into());
    let first = workbench.prepare_for(7, "toolbar:1", "context", 1).unwrap();
    let rejected = workbench
        .reject_prepared(7, &first.request_id, "context", 2)
        .unwrap();
    assert_eq!(rejected.status, ReceiptStatus::Rejected);
    assert_eq!(
        workbench
            .prepare_for(7, "toolbar:1", "context", 3)
            .unwrap()
            .status,
        ReceiptStatus::Rejected
    );
    for sequence in 0..(super::super::MAX_RECEIPTS - 1) {
        workbench
            .prepare_for(7, &format!("toolbar:{sequence}:other"), "context", 3)
            .unwrap();
    }
    workbench
        .prepare_for(7, "toolbar:next", "context", 3)
        .unwrap();
    assert_eq!(
        workbench
            .prepare_for(7, "toolbar:1", "context", 3)
            .unwrap()
            .status,
        ReceiptStatus::Retired
    );
}

#[test]
fn prepare_rejects_correlation_reused_for_another_context() {
    let workbench = PreviewWorkbench::with_epoch("epoch".into());
    workbench.prepare_for(7, "toolbar:1", "first", 1).unwrap();
    assert_eq!(
        workbench
            .prepare_for(7, "toolbar:1", "second", 1)
            .unwrap_err(),
        "prepare_context_mismatch"
    );
}

#[test]
fn prepare_replay_survives_the_correlation_capacity_limit() {
    let workbench = PreviewWorkbench::with_epoch("epoch".into());
    let original = workbench
        .prepare_for(7, "toolbar:existing", "context", 1)
        .unwrap();
    let mut tasks = workbench.tasks.lock().unwrap();
    let task = tasks.get_mut(&7).unwrap();
    for index in 0..super::super::MAX_PREPARE_CORRELATIONS {
        task.retired_prepares.insert(
            format!("retired:{index}"),
            ("hash".into(), format!("retired:{index}")),
        );
    }
    drop(tasks);

    assert_eq!(
        workbench
            .prepared_for(7, "toolbar:existing", "context", 2)
            .unwrap()
            .unwrap()
            .request_id,
        original.request_id
    );
    assert_eq!(
        workbench
            .prepare_for(7, "toolbar:new", "context", 2)
            .unwrap_err(),
        "receipt_capacity"
    );
}
