use praxis_lib::preview_workbench::{PreviewWorkbench, ReceiptStatus};

fn workbench() -> PreviewWorkbench {
    PreviewWorkbench::with_epoch("epoch-a".into())
}

#[test]
fn accepted_receipt_binds_context_and_cannot_start_twice() {
    let receipts = workbench();
    let request = receipts
        .prepare(7, "manual\nhttp://127.0.0.1:3000\nask", 10)
        .unwrap();

    let first = receipts
        .accept(
            7,
            &request.request_id,
            "manual\nhttp://127.0.0.1:3000\nask",
            10,
        )
        .unwrap();
    let duplicate = receipts
        .accept(
            7,
            &request.request_id,
            "manual\nhttp://127.0.0.1:3000\nask",
            10,
        )
        .unwrap();

    assert_eq!(first.status, ReceiptStatus::Accepted);
    assert_eq!(duplicate.status, ReceiptStatus::Accepted);
    assert!(receipts
        .accept(
            7,
            &request.request_id,
            "manual\nhttp://127.0.0.1:4000\nask",
            10
        )
        .is_err());
}

#[test]
fn completed_prefix_retires_without_reusing_a_request_id() {
    let receipts = workbench();
    let first = receipts.prepare(7, "first", 10).unwrap();
    receipts.accept(7, &first.request_id, "first", 10).unwrap();
    receipts.finish(7, &first.request_id, "convo:7:11".into());
    let second = receipts.prepare(7, "second", 12).unwrap();

    assert_eq!(
        receipts.receipt(7, &first.request_id).status,
        ReceiptStatus::Finished
    );
    assert_ne!(first.request_id, second.request_id);
}

#[test]
fn full_unretirable_window_rejects_new_preparation() {
    let receipts = workbench();

    for seq in 0..128 {
        receipts.prepare(7, &format!("message-{seq}"), 10).unwrap();
    }

    assert_eq!(
        receipts.prepare(7, "overflow", 10).unwrap_err(),
        "receipt_capacity"
    );
}

#[test]
fn finished_prefix_retires_only_when_a_new_receipt_needs_room() {
    let receipts = workbench();
    let first = receipts.prepare(7, "first", 10).unwrap();
    receipts.accept(7, &first.request_id, "first", 10).unwrap();
    receipts.finish(7, &first.request_id, "convo:7:11".into());
    for seq in 0..127 {
        receipts.prepare(7, &format!("message-{seq}"), 12).unwrap();
    }

    let next = receipts.prepare(7, "next", 13).unwrap();

    assert_eq!(
        receipts.receipt(7, &first.request_id).status,
        ReceiptStatus::Retired
    );
    assert_eq!(next.status, ReceiptStatus::Prepared);
}

#[test]
fn expired_prepared_receipt_is_rejected_on_lookup() {
    let receipts = workbench();
    let request = receipts.prepare(7, "old", 10).unwrap();
    let next = receipts.prepare(7, "new", 10 + 86_400).unwrap();

    assert_eq!(
        receipts
            .receipt_at(7, &request.request_id, 10 + 86_400)
            .status,
        ReceiptStatus::Rejected
    );
    assert_eq!(next.status, ReceiptStatus::Prepared);
}

#[test]
fn invalidated_task_rejects_late_acceptance() {
    let receipts = workbench();
    let request = receipts.prepare(7, "ask", 10).unwrap();
    receipts.invalidate(7);

    assert_eq!(
        receipts
            .accept(7, &request.request_id, "ask", 10)
            .unwrap()
            .status,
        ReceiptStatus::Invalidated
    );
}

#[test]
fn finish_updates_only_its_bound_request() {
    let receipts = workbench();
    let first = receipts.prepare(7, "first", 10).unwrap();
    let second = receipts.prepare(7, "second", 10).unwrap();
    receipts.accept(7, &first.request_id, "first", 10).unwrap();
    receipts.accept(7, &second.request_id, "second", 10).unwrap();

    receipts.finish(7, &second.request_id, "convo:7:11".into());

    assert_eq!(receipts.receipt(7, &first.request_id).status, ReceiptStatus::Accepted);
    assert_eq!(receipts.receipt(7, &second.request_id).status, ReceiptStatus::Finished);
}


#[test]
fn unissued_or_foreign_request_ids_are_invalidated() {
    let receipts = workbench();
    let request = receipts.prepare(7, "ask", 10).unwrap();
    let foreign = request.request_id.replacen("epoch-a", "epoch-b", 1);
    let unissued = request.request_id.replacen(":1:", ":2:", 1);

    assert_eq!(
        receipts.receipt(7, &foreign).status,
        ReceiptStatus::Invalidated
    );
    assert_eq!(
        receipts.receipt(7, &unissued).status,
        ReceiptStatus::Invalidated
    );
}
