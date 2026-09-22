use super::*;

mod database;

#[test]
fn classifies_zero_one_and_multiple_done_candidates_without_guessing() {
    assert_eq!(
        classify_selection(["AwaitingReview", "Discarded"]),
        (EnsembleSelectionStatus::Pending, None)
    );
    assert_eq!(
        classify_selection(["Discarded", "Done", "AwaitingReview"]),
        (EnsembleSelectionStatus::Selected, Some(1))
    );
    assert_eq!(
        classify_selection(["Done", "Discarded", "Done"]),
        (EnsembleSelectionStatus::Ambiguous, None)
    );
}
