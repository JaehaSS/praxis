use super::*;
use crate::evidence::model::Observation;

#[tokio::test]
async fn stale_observation_generation_is_discarded_without_a_check_receipt() {
    let path =
        crate::testtmp::dir().join(format!("praxis-evidence-cas-{}.sqlite", std::process::id()));
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    crate::memory::migrate(&pool).await.unwrap();
    let memory_id = crate::memory::create_candidate(
        &pool,
        crate::memory::tier::PROJECT,
        Some("/repo"),
        crate::memory::knowledge_type::CLAIM,
        "CAS generation",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    crate::memory::add_user_confirmation(&pool, memory_id, 100, None)
        .await
        .unwrap();
    let evidence = crate::evidence::list_evidence(&pool, memory_id)
        .await
        .unwrap();
    let loaded = LoadedEvidence {
        memory_id,
        version: 1,
        memory_status: crate::memory::knowledge_status::CANDIDATE.into(),
        scope_key: Some("/repo".into()),
        evidence: evidence.clone(),
    };
    sqlx::query("UPDATE memory_evidence SET checked_at = 101 WHERE id = ?")
        .bind(evidence[0].id)
        .execute(&pool)
        .await
        .unwrap();
    let observations = vec![Observation {
        evidence: evidence[0].clone(),
        status: crate::memory::evidence_status::VALID.into(),
        observed_hash: None,
    }];
    assert!(commit(&pool, memory_id, &loaded, &observations, 102)
        .await
        .unwrap()
        .is_none());
    let checks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memory_evidence_checks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(checks, 0);
    let _ = std::fs::remove_file(path);
}
