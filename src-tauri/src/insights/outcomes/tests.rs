use super::*;

mod database;
mod no_reexplanation;

#[test]
fn cutoff_supports_seven_thirty_and_all_ranges() {
    let now = 10_000_000;

    assert_eq!(cutoff("7d", now), now - 7 * 86_400);
    assert_eq!(cutoff("30d", now), now - 30 * 86_400);
    assert_eq!(cutoff("all", now), 0);
    assert_eq!(cutoff("unknown", now), 0);
}
