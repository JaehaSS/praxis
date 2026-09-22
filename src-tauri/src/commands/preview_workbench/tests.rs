use super::*;

#[test]
fn accepted_receipt_replays_after_page_navigation_without_skipping_context_check() {
    let workbench = crate::preview_workbench::PreviewWorkbench::with_epoch("epoch".into());
    let original = context("ask", "http://127.0.0.1:3000", &PreviewSource::Manual);
    let request = workbench
        .prepare(7, &original, crate::commands::now())
        .unwrap();
    workbench
        .accept(7, &request.request_id, &original, 10)
        .unwrap();

    let replay = existing_receipt(&workbench, 7, &request.request_id, &original)
        .unwrap()
        .unwrap();
    assert_eq!(replay.status, ReceiptStatus::Accepted);

    let changed = context("ask", "http://127.0.0.1:4000", &PreviewSource::Manual);
    assert!(existing_receipt(&workbench, 7, &request.request_id, &changed).is_err());
}

#[test]
fn changed_preview_rejects_only_the_issued_prepared_receipt() {
    let workbench = crate::preview_workbench::PreviewWorkbench::with_epoch("epoch".into());
    let original = context("ask", "http://127.0.0.1:3000", &PreviewSource::Manual);
    let request = workbench
        .prepare(7, &original, crate::commands::now())
        .unwrap();
    let changed = context("ask", "http://127.0.0.1:4000", &PreviewSource::Manual);

    assert!(reject_changed_preview(&workbench, 7, &request.request_id, &changed).is_err());
    assert_eq!(
        workbench.receipt(7, &request.request_id).status,
        ReceiptStatus::Prepared
    );
    let rejected = reject_changed_preview(&workbench, 7, &request.request_id, &original).unwrap();
    workbench.finish(7, &request.request_id, "convo:7:11".into());

    assert_eq!(rejected.status, ReceiptStatus::Rejected);
    assert_eq!(
        workbench.receipt(7, &request.request_id).status,
        ReceiptStatus::Rejected
    );
}

#[test]
fn prepared_correlation_replays_before_a_live_preview_url_check() {
    let workbench = crate::preview_workbench::PreviewWorkbench::with_epoch("epoch".into());
    let original = context("ask", "http://127.0.0.1:3000", &PreviewSource::Manual);
    let prepared = workbench
        .prepare_for(7, "toolbar:lost", &original, 1)
        .unwrap();

    let replay = workbench
        .prepared_for(7, "toolbar:lost", &original, 2)
        .unwrap()
        .unwrap();

    assert_eq!(replay.request_id, prepared.request_id);
    assert_eq!(replay.status, ReceiptStatus::Prepared);
}
