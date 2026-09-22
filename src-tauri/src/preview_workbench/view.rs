use super::{ReceiptStatus, ReceiptView};

pub(super) fn view(
    request_id: String,
    status: ReceiptStatus,
    result_id: Option<String>,
) -> ReceiptView {
    ReceiptView {
        request_id,
        accepted: matches!(status, ReceiptStatus::Accepted | ReceiptStatus::Finished),
        running: status == ReceiptStatus::Accepted,
        retryable: status == ReceiptStatus::Prepared,
        status,
        result_id,
    }
}

pub(super) fn retired_view(request_id: String) -> ReceiptView {
    view(request_id, ReceiptStatus::Retired, None)
}

pub(super) fn invalidated_view(request_id: String) -> ReceiptView {
    view(request_id, ReceiptStatus::Invalidated, None)
}
