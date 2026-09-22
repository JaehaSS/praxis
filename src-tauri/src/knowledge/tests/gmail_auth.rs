//! Gmail OAuth — PKCE·인증 URL·루프백 콜백 검증.
//!
//! 실제 Google 서버를 타지 않는다. 여기서 고정하는 것은 **우리 쪽 조립이 맞는가**이고,
//! 그게 틀리면 나타나는 증상이 하나같이 "왜인지 모르게 인증이 안 됨"이라 값이 크다.

use crate::knowledge::source::gmail_auth::*;

#[test]
fn pkce_verifier_meets_the_length_floor() {
    let (verifier, _) = pkce_pair().unwrap();
    // RFC 7636 §4.1 — 43~128자. 32바이트 base64url이면 정확히 43자다.
    assert!(
        (43..=128).contains(&verifier.len()),
        "verifier 길이가 규격을 벗어났다: {}",
        verifier.len()
    );
}

/// 표준 base64로 인코딩하면 Google이 challenge 불일치로 거부하는데, 그 실패는
/// "코드 교환 실패"로만 보여 원인을 찾기 어렵다. 문자 집합을 여기서 고정한다.
#[test]
fn pkce_challenge_is_url_safe_and_unpadded() {
    let (verifier, challenge) = pkce_pair().unwrap();
    for banned in ['=', '+', '/'] {
        assert!(
            !challenge.contains(banned),
            "challenge에 '{banned}'가 있다: {challenge}"
        );
        assert!(
            !verifier.contains(banned),
            "verifier에 '{banned}'가 있다: {verifier}"
        );
    }
}

#[test]
fn pkce_challenge_is_deterministic_but_pairs_are_not() {
    let (verifier, challenge) = pkce_pair().unwrap();
    assert_eq!(challenge_for(&verifier), challenge, "challenge가 결정적이지 않다");

    let (other, _) = pkce_pair().unwrap();
    assert_ne!(verifier, other, "매번 같은 verifier가 나온다 — 난수가 아니다");
}

/// **이 둘이 빠지면 두 번째 인증부터 refresh token이 오지 않는다.** 재인증을 해도
/// 아무것도 나아지지 않는, 원인을 짐작하기 어려운 상태가 된다.
#[test]
fn authorize_url_always_requests_offline_access_with_forced_consent() {
    let url = authorize_url("cid", "http://127.0.0.1:1234", "chal", "st").unwrap();
    assert!(url.contains("access_type=offline"), "offline이 없다: {url}");
    assert!(url.contains("prompt=consent"), "consent 강제가 없다: {url}");
}

#[test]
fn authorize_url_pins_s256_and_readonly_scope() {
    let url = authorize_url("cid", "http://127.0.0.1:1234", "chal", "st").unwrap();
    assert!(url.contains("code_challenge_method=S256"), "S256이 아니다: {url}");
    // scope는 하나뿐이어야 한다. 넓히면 restricted scope 검증 부담만 커진다.
    assert!(url.contains("gmail.readonly"), "scope가 없다: {url}");
    assert!(
        !url.contains("gmail.modify") && !url.contains("mail.google.com"),
        "읽기 외 권한이 섞였다: {url}"
    );
}

/// 판별이 틀리면 네트워크 장애에 "재인증하세요"를 띄우거나, 반대로 죽은 토큰을
/// 영원히 재시도한다. 양쪽 다 픽스처로 고정한다.
#[test]
fn token_errors_split_into_reauth_and_transient() {
    let revoked = r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#;
    assert!(matches!(
        classify_token_error(400, revoked),
        AuthError::NeedsReauth(_)
    ));

    assert!(matches!(
        classify_token_error(503, "backend error"),
        AuthError::Transient(_)
    ));
    assert!(matches!(
        classify_token_error(429, "rate limit"),
        AuthError::Transient(_)
    ));
}

#[tokio::test]
async fn loopback_binds_to_localhost_only() {
    let (listener, redirect_uri) = bind_loopback().await.unwrap();
    let addr = listener.local_addr().unwrap();
    // 0.0.0.0에 열면 같은 네트워크의 다른 기기가 인증 코드를 가로챌 수 있다.
    assert!(addr.ip().is_loopback(), "루프백이 아닌 주소에 바인딩했다: {addr}");
    assert_ne!(addr.port(), 0, "포트가 할당되지 않았다");
    assert!(redirect_uri.starts_with("http://127.0.0.1:"), "{redirect_uri}");
}

#[tokio::test]
async fn loopback_receives_the_authorization_code() {
    let (listener, redirect_uri) = bind_loopback().await.unwrap();
    let waiting = tokio::spawn(async move {
        wait_for_code(listener, "state-abc", std::time::Duration::from_secs(5)).await
    });

    let response = reqwest::get(format!("{redirect_uri}/?code=auth-code-1&state=state-abc"))
        .await
        .unwrap();
    assert!(response.status().is_success());

    let code = waiting.await.unwrap().unwrap();
    assert_eq!(code, "auth-code-1");
}

/// state가 없으면 공격자가 자기 인증 코드를 사용자 브라우저로 흘려보내
/// **남의 메일함을 사용자 계정에 붙일 수** 있다.
#[tokio::test]
async fn loopback_rejects_a_mismatched_state() {
    let (listener, redirect_uri) = bind_loopback().await.unwrap();
    let waiting = tokio::spawn(async move {
        wait_for_code(listener, "state-abc", std::time::Duration::from_secs(5)).await
    });

    let _ = reqwest::get(format!("{redirect_uri}/?code=attacker&state=wrong")).await;

    let outcome = waiting.await.unwrap();
    let message = outcome.unwrap_err().to_string();
    assert!(message.contains("state"), "state 불일치를 막지 않았다: {message}");
}

/// 사용자가 동의 화면에서 거부한 경우. 코드가 없는데 성공으로 처리하면
/// 그 뒤 토큰 교환이 알 수 없는 실패로 이어진다.
#[tokio::test]
async fn loopback_surfaces_a_denied_consent() {
    let (listener, redirect_uri) = bind_loopback().await.unwrap();
    let waiting = tokio::spawn(async move {
        wait_for_code(listener, "state-abc", std::time::Duration::from_secs(5)).await
    });

    let _ = reqwest::get(format!("{redirect_uri}/?error=access_denied&state=state-abc")).await;

    let message = waiting.await.unwrap().unwrap_err().to_string();
    assert!(message.contains("거부"), "거부를 표면화하지 않았다: {message}");
}

// ── client 해석 — 번들 vs 사용자 입력 (ADR 0147) ──

fn bundled_client() -> Credentials {
    Credentials {
        client_id: "org.apps.googleusercontent.com".into(),
        client_secret: "GOCSPX-org".into(),
        origin: CredentialOrigin::Bundled,
    }
}

/// 번들을 주입하지 않고 빌드하면 예전과 똑같이 BYO만 남아야 한다.
#[test]
fn without_a_bundle_the_user_must_supply_both() {
    assert_eq!(
        resolve_with(None, "", None).unwrap_err(),
        ResolveError::NoClientId
    );
    assert_eq!(
        resolve_with(None, "mine.apps.googleusercontent.com", None).unwrap_err(),
        ResolveError::NoClientSecret
    );

    let resolved = resolve_with(None, "mine.apps.googleusercontent.com", Some("GOCSPX-mine")).unwrap();
    assert_eq!(resolved.origin, CredentialOrigin::User);
    assert_eq!(resolved.client_id, "mine.apps.googleusercontent.com");
    assert_eq!(resolved.client_secret, "GOCSPX-mine");
}

/// 팀원이 아무것도 입력하지 않은 상태 — 여기가 원클릭이 성립하는 지점이다.
#[test]
fn an_empty_form_falls_back_to_the_bundled_client() {
    let resolved = resolve_with(Some(bundled_client()), "", None).unwrap();
    assert_eq!(resolved.origin, CredentialOrigin::Bundled);
    assert_eq!(resolved.client_id, "org.apps.googleusercontent.com");
}

/// 번들된 client가 받아주지 않는 계정이 있다. 번들이 있다고 사용자 값을 밀어내면
/// 그런 계정을 붙일 길이 통째로 사라진다.
#[test]
fn a_user_client_outranks_the_bundle() {
    let resolved = resolve_with(
        Some(bundled_client()),
        "mine.apps.googleusercontent.com",
        Some("GOCSPX-mine"),
    )
    .unwrap();
    assert_eq!(resolved.origin, CredentialOrigin::User);
    assert_eq!(resolved.client_id, "mine.apps.googleusercontent.com");
    assert_eq!(resolved.client_secret, "GOCSPX-mine");
}

/// **핵심 불변식** — 두 출처를 섞지 않는다.
///
/// 사용자 client_id에 번들 secret이 붙으면 Google은 `invalid_client`를 주고, 그 실패는
/// 화면에 "재인증이 필요합니다"로만 보인다. 사용자는 자기 secret을 안 넣었다는 사실에
/// 영영 도달하지 못한다. 섞느니 "secret이 없다"고 바로 말하는 편이 낫다.
#[test]
fn a_user_client_never_borrows_the_bundled_secret() {
    assert_eq!(
        resolve_with(Some(bundled_client()), "mine.apps.googleusercontent.com", None).unwrap_err(),
        ResolveError::NoClientSecret
    );
    assert_eq!(
        resolve_with(Some(bundled_client()), "mine.apps.googleusercontent.com", Some("   ")).unwrap_err(),
        ResolveError::NoClientSecret
    );
}

/// 붙여넣기는 공백을 흔히 달고 온다. 공백뿐인 값은 "입력하지 않음"으로 읽어야
/// 번들 폴백이 살아난다 — 아니면 팀원이 실수로 스페이스 하나를 남기고 연결에 실패한다.
#[test]
fn whitespace_only_input_counts_as_absent() {
    let resolved = resolve_with(Some(bundled_client()), "   ", None).unwrap();
    assert_eq!(resolved.origin, CredentialOrigin::Bundled);

    assert_eq!(
        resolve_with(None, "  \t ", None).unwrap_err(),
        ResolveError::NoClientId
    );
}

/// 값 주위 공백은 잘라서 저장한다. `client_id`에 개행이 남으면 authorize URL이
/// 조용히 깨진다.
#[test]
fn resolved_values_are_trimmed() {
    let resolved = resolve_with(None, "  mine.apps.googleusercontent.com  ", Some(" GOCSPX-mine ")).unwrap();
    assert_eq!(resolved.client_id, "mine.apps.googleusercontent.com");
    assert_eq!(resolved.client_secret, "GOCSPX-mine");
}

/// 빌드 타임 주입이 실제로 상수에 닿는가.
///
/// `resolve_with`는 판정 규칙만 고정할 뿐 `option_env!`가 값을 물어 오는지는 말해주지
/// 않는다. 주입해도 상수가 비어 있으면 팀원 화면은 조용히 예전(BYO)으로 돌아가는데,
/// 그 증상은 "왜 아직도 입력하라고 하지"로만 보인다.
///
/// 주입 없이 빌드하는 것도 정상이므로 두 경우를 다 허용하되, **주입했는데 안 잡히는
/// 상태**는 걸러낸다. CI에서 env 유무로 두 번 돌리면 양쪽 경로가 모두 실증된다.
#[test]
fn the_build_time_injection_reaches_resolve() {
    // 컴파일 타임(`option_env!`)과 런타임(`std::env::var`)을 대조한다. 같은 env로 빌드하고
    // 실행했다면 두 값이 일치해야 하고, 어긋나면 재빌드가 일어나지 않았다는 뜻이다.
    let injected = std::env::var("PRAXIS_GMAIL_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match (bundled(), injected) {
        (Some(credentials), expected) => {
            assert_eq!(credentials.origin, CredentialOrigin::Bundled);
            assert!(!credentials.client_secret.is_empty());
            if let Some(expected) = expected {
                assert_eq!(
                    credentials.client_id, expected,
                    "주입한 값이 상수에 닿지 않았다 — build.rs의 rerun-if-env-changed를 확인하라"
                );
            }
            // 주입이 있으면 빈 폼이 번들로 떨어진다 — 여기가 원클릭이 성립하는 지점이다.
            let resolved = resolve("", None).unwrap();
            assert_eq!(resolved.origin, CredentialOrigin::Bundled);
            assert_eq!(resolved.client_id, credentials.client_id);
        }
        (None, None) => {
            // 주입이 없으면 예전과 같이 BYO만 남는다.
            assert_eq!(resolve("", None).unwrap_err(), ResolveError::NoClientId);
        }
        (None, Some(value)) => panic!(
            "env를 주입했는데 상수가 비어 있다 — 재빌드가 일어나지 않았다: {value}"
        ),
    }
}
