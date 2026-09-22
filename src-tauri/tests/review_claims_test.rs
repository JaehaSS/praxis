use praxis_lib::review_ops::ReviewClaims;

#[test]
fn duplicate_review_claims_and_finalization_conflicts_are_fail_closed() {
    let claims = ReviewClaims::default();
    let verify = claims.claim_verify(7).expect("first verify claim");

    assert!(
        claims.claim_verify(7).is_err(),
        "duplicate verify must fail"
    );
    assert!(
        claims.claim_finalization(7).is_err(),
        "finalization must wait for every review operation"
    );

    drop(verify);

    let finalization = claims
        .claim_finalization(7)
        .expect("finalization after review cleanup");
    assert!(claims.claim_verify(7).is_err());
    drop(finalization);

    assert!(claims.claim_verify(7).is_ok(), "drop must release claims");
}

#[test]
fn claims_are_isolated_by_task() {
    let claims = ReviewClaims::default();
    let _verify = claims.claim_verify(7).unwrap();

    assert!(claims.claim_finalization(8).is_ok());
}
