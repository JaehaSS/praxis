//! Web Push VAPID·구독 관리 (설계 0013 §8)

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use praxis_lib::db;
use praxis_lib::runner::push::{self, VapidKeys};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[test]
fn audience는_push_서비스_origin만_남긴다() {
    // VAPID JWT의 aud가 틀리면 push 서비스가 401로 거절한다.
    assert_eq!(
        push::audience("https://fcm.googleapis.com/fcm/send/abc123"),
        Some("https://fcm.googleapis.com".to_string())
    );
    assert_eq!(
        push::audience("https://web.push.apple.com/QRSTUV"),
        Some("https://web.push.apple.com".to_string())
    );
    assert_eq!(
        push::audience("https://example.com"),
        Some("https://example.com".to_string())
    );
    assert_eq!(push::audience("not-a-url"), None);
    assert_eq!(push::audience("https://"), None);
}

#[test]
fn 종료_전이에만_알린다() {
    // 출력 이벤트마다 폰을 깨우면 알림이 무의미해진다.
    assert!(push::should_notify("completed"));
    assert!(push::should_notify("failed"));
    assert!(push::should_notify("cancelled"));
    assert!(!push::should_notify("output"));
    assert!(!push::should_notify("queued"));
    assert!(!push::should_notify("running"));
}

#[test]
fn vapid_키는_저장되고_다시_읽어도_같은_공개키다() {
    let path = temp_path("vapid");
    let first = VapidKeys::load_or_create(&path).unwrap();
    let second = VapidKeys::load_or_create(&path).unwrap();
    assert_eq!(first.public_key_base64url(), second.public_key_base64url());

    // 브라우저 applicationServerKey는 비압축 P-256 포인트(65바이트, 0x04 접두)여야 한다.
    let decoded = URL_SAFE_NO_PAD
        .decode(first.public_key_base64url())
        .unwrap();
    assert_eq!(decoded.len(), 65);
    assert_eq!(decoded[0], 0x04);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn authorization_헤더는_vapid_형식을_따른다() {
    let path = temp_path("vapid-auth");
    let keys = VapidKeys::load_or_create(&path).unwrap();
    let header = keys
        .authorization("https://fcm.googleapis.com", 1_800_000_000)
        .unwrap();

    let rest = header
        .strip_prefix("vapid t=")
        .expect("vapid 스킴이어야 한다");
    let (jwt, key_part) = rest.split_once(", k=").expect("k 파라미터가 있어야 한다");
    assert_eq!(key_part, keys.public_key_base64url());

    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3, "JWT는 3부분이어야 한다");

    let header_json: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header_json["alg"], "ES256");

    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["aud"], "https://fcm.googleapis.com");
    assert!(claims["exp"].as_i64().unwrap() > 1_800_000_000);
    assert!(claims["sub"].as_str().unwrap().starts_with("mailto:"));

    // ES256 서명은 DER이 아니라 r||s 고정 64바이트다 — 여기가 틀리면 모든 발송이 401이 된다.
    assert_eq!(URL_SAFE_NO_PAD.decode(parts[2]).unwrap().len(), 64);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn 구독은_endpoint_기준으로_갱신되고_세션과_함께_사라진다() {
    let path = temp_path("push.sqlite");
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    push::migrate(&pool).await.unwrap();

    push::subscribe(&pool, "https://push.example/a", Some(7), 100)
        .await
        .unwrap();
    // 같은 endpoint 재구독은 중복 행이 아니라 갱신이어야 한다.
    push::subscribe(&pool, "https://push.example/a", Some(7), 200)
        .await
        .unwrap();
    push::subscribe(&pool, "https://push.example/b", Some(8), 200)
        .await
        .unwrap();
    assert_eq!(push::endpoints(&pool).await.unwrap().len(), 2);

    push::unsubscribe(&pool, "https://push.example/b")
        .await
        .unwrap();
    assert_eq!(
        push::endpoints(&pool).await.unwrap(),
        vec!["https://push.example/a"]
    );

    // 기기를 회수하면 그 기기의 구독도 사라져야 한다 — 남으면 회수한 폰이 계속 알림을 받는다.
    assert_eq!(push::remove_for_session(&pool, 7).await.unwrap(), 1);
    assert!(push::endpoints(&pool).await.unwrap().is_empty());

    let _ = std::fs::remove_file(&path);
}

fn temp_path(label: &str) -> std::path::PathBuf {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir().join(format!(
        "praxis-push-{label}-{}-{suffix}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}
