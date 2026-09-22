//! 벤더 CLI의 인증 상태·설치 버전·업데이트 가용성 스냅샷.
//!
//! `usage`와 역할이 갈린다 — **usage는 잔량, 여기는 인증·버전.** 겹치지 않는다.
//!
//! 설계: `docs/designs/0051.2026-08-23-agent-auth-update-in-app-design.md`

pub mod action;
pub mod antigravity;
pub mod autoupdate;
pub mod detect;

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use detect::{AuthState, InstallMethod};

/// npm registry 재조회 최소 간격(초). 매 렌더마다 네트워크를 타면 안 된다.
const LATEST_TTL: i64 = 6 * 3600;

/// 이 모듈이 다루는 벤더 — (key, 표시명, 실행 파일, npm 패키지).
///
/// Antigravity(`agy`)는 인증 상태를 노출하는 표면이 확인되지 않아 제외한다.
/// `usage`가 같은 이유로 `unsupported` 고정인 것과 같다.
const VENDORS: &[(&str, &str, &str, &str)] = &[
    (
        "claude",
        "Claude Code",
        "claude",
        "@anthropic-ai/claude-code",
    ),
    ("codex", "Codex", "codex", "@openai/codex"),
];

/// 벤더 한 곳의 인증·버전 상태.
#[derive(Debug, Clone, Serialize)]
pub struct VendorHealth {
    /// `agent::PRESETS`의 key — "claude" | "codex".
    pub vendor: String,
    pub label: String,
    pub auth: AuthState,
    /// 상태 부연(툴팁). CLI가 준 문자열을 그대로 흘린다.
    pub auth_detail: Option<String>,
    /// 계정 식별자(이메일 등). 소스가 줄 때만.
    pub account: Option<String>,
    /// 구독 플랜 문자열.
    pub plan: Option<String>,
    /// 설치된 버전. 실행 파일을 못 찾으면 None.
    pub installed: Option<String>,
    /// registry가 아는 최신 버전. 조회 실패면 None.
    pub latest: Option<String>,
    pub update_available: bool,
    pub install_method: InstallMethod,
    /// 실행 파일 실제 경로 — `Unknown` 판정 시 사용자가 눈으로 확인할 근거.
    pub bin_path: Option<String>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthSnapshot {
    pub vendors: Vec<VendorHealth>,
    pub checked_at: i64,
}

/// (vendor, 조회 시각, 최신 버전) — const 초기화 Mutex, `usage::OAUTH_CACHE`와 같은 패턴.
static LATEST_CACHE: Mutex<Vec<(String, i64, Option<String>)>> = Mutex::new(Vec::new());

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

fn cached_latest(vendor: &str, now: i64) -> Option<Option<String>> {
    let cache = LATEST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.iter().find_map(|(key, at, value)| {
        (key == vendor && now - *at < LATEST_TTL).then(|| value.clone())
    })
}

fn store_latest(vendor: &str, now: i64, value: Option<String>) {
    let mut cache = LATEST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|(key, _, _)| key != vendor);
    cache.push((vendor.to_string(), now, value));
}

/// npm registry에서 최신 버전을 읽는다. 실패는 전부 None — 업데이트 배지만 안 뜬다.
async fn fetch_latest(package: &str) -> Option<String> {
    // scoped 패키지는 `/`를 인코딩해야 한다: @openai/codex → @openai%2Fcodex
    let encoded = package.replace('/', "%2F");
    let url = format!("https://registry.npmjs.org/{encoded}/latest");
    let response = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    body.get("version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

/// 벤더 하나의 로컬 상태(인증·버전·설치 방식). 프로세스를 띄우므로 blocking.
fn local_health(vendor: &str, label: &str, bin: &str, now: i64) -> VendorHealth {
    let bin_path = detect::resolve_bin(bin);
    // 설정 패널을 열 때마다 CLI에 직접 묻는다(캐시를 읽지 않는다) — 로그인 직후 새로고침이
    // 옛 값을 돌려주면 이 기능의 존재 이유가 없어진다. 다만 결과는 캐시에 남겨 preflight가 쓴다.
    let (auth, account, detail) = detect::auth_of(vendor);
    store_auth(vendor, now, auth);
    // claude는 (state, account, plan), codex는 (state, None, detail)로 돌아온다.
    let (plan, auth_detail) = if vendor == "claude" {
        (detail, None)
    } else {
        (None, detail)
    };
    VendorHealth {
        vendor: vendor.to_string(),
        label: label.to_string(),
        auth,
        auth_detail,
        account,
        plan,
        installed: detect::installed_version(bin),
        latest: None,
        update_available: false,
        install_method: bin_path
            .as_deref()
            .map_or(InstallMethod::Unknown, detect::install_method_of),
        bin_path,
        checked_at: now,
    }
}

/// 전 벤더 스냅샷. 로컬 조사는 blocking 풀에서, registry 조회는 캐시를 앞세운다.
///
/// 로컬 조사(인증·버전)는 **항상 다시 한다** — 캐시하는 것은 registry 왕복뿐이다.
/// 로그인 직후 새로고침이 옛 상태를 돌려주면 이 기능의 존재 이유가 없어진다.
pub async fn snapshot(force: bool) -> HealthSnapshot {
    let now = now_secs();
    let mut vendors = Vec::with_capacity(VENDORS.len());
    for (vendor, label, bin, package) in VENDORS {
        let (owned_vendor, owned_label, owned_bin) =
            ((*vendor).to_string(), (*label).to_string(), (*bin).to_string());
        let mut health = tokio::task::spawn_blocking(move || {
            local_health(&owned_vendor, &owned_label, &owned_bin, now)
        })
        .await
        .unwrap_or_else(|_| unavailable(vendor, label, now));

        let cached = if force {
            None
        } else {
            cached_latest(&health.vendor, now)
        };
        let latest = match cached {
            Some(cached) => cached,
            None => {
                let fetched = fetch_latest(package).await;
                store_latest(&health.vendor, now, fetched.clone());
                fetched
            }
        };
        health.update_available = match (health.installed.as_deref(), latest.as_deref()) {
            (Some(installed), Some(latest)) => detect::is_newer(installed, latest),
            _ => false,
        };
        health.latest = latest;
        vendors.push(health);
    }
    HealthSnapshot {
        vendors,
        checked_at: now,
    }
}

/// 인증 상태 캐시 수명(초). preflight가 작업마다 CLI를 띄우면 큐가 눈에 띄게 느려진다.
/// 짧게 잡는 이유는 로그인 직후 캐시가 남아 작업을 계속 막는 상황을 피하기 위해서다.
const AUTH_TTL: i64 = 60;

static AUTH_CACHE: Mutex<Vec<(String, i64, AuthState)>> = Mutex::new(Vec::new());

fn store_auth(vendor: &str, now: i64, state: AuthState) {
    let mut cache = AUTH_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|(key, _, _)| key != vendor);
    cache.push((vendor.to_string(), now, state));
}

fn cached_auth(vendor: &str, now: i64) -> Option<AuthState> {
    let cache = AUTH_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .iter()
        .find_map(|(key, at, state)| (key == vendor && now - *at < AUTH_TTL).then_some(*state))
}

/// 캐시를 통째로 버린다 — 로그인·로그아웃 직후처럼 상태가 확실히 바뀐 시점에 부른다.
pub fn invalidate_auth_cache() {
    AUTH_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// preflight용 인증 상태. 캐시가 신선하면 그것을, 아니면 CLI에 묻고 캐시에 남긴다.
pub fn auth_state_cached(vendor: &str) -> AuthState {
    let now = now_secs();
    if let Some(state) = cached_auth(vendor, now) {
        return state;
    }
    let (state, _, _) = detect::auth_of(vendor);
    store_auth(vendor, now, state);
    state
}

/// 작업의 agent가 인증 때문에 지금 시작할 수 없다면 그 벤더를 돌려준다.
///
/// `Unknown`은 막지 않는다 — 확인에 실패한 것을 로그아웃으로 단정하면 멀쩡한 큐가 선다.
pub fn blocking_vendor(agent: Option<&str>) -> Option<String> {
    let agent = agent?.trim();
    // 이 모듈이 상태를 아는 벤더만 대상이다. 커스텀 명령·다른 벤더는 통과시킨다.
    bin_of(agent)?;
    (auth_state_cached(agent) == AuthState::LoggedOut).then(|| agent.to_string())
}

/// 로그인이 회복된 벤더의 차단을 푼다. 반환: 풀려난 작업 수.
///
/// 캐시를 먼저 버린다 — 방금 로그인한 결과를 보려는 호출인데 옛 값을 보면 아무것도 안 풀린다.
pub async fn reconcile_blocks(pool: &sqlx::SqlitePool) -> anyhow::Result<u64> {
    invalidate_auth_cache();
    let mut freed = 0;
    for (vendor, _, _, _) in VENDORS {
        let vendor = (*vendor).to_string();
        let state = tokio::task::spawn_blocking({
            let vendor = vendor.clone();
            move || auth_state_cached(&vendor)
        })
        .await
        .unwrap_or(AuthState::Unknown);
        // Unknown에서도 푼다 — 막아둔 근거(LoggedOut)가 더는 확실하지 않으면,
        // 계속 세워두는 것보다 한 번 더 시도해 보는 편이 낫다. 실패하면 다시 막힌다.
        if state != AuthState::LoggedOut {
            freed += crate::db::clear_auth_block(pool, &vendor).await?;
        }
    }
    Ok(freed)
}

/// 벤더의 npm 패키지명 — 업데이트 명령 조립에 쓰인다.
pub fn package_of(vendor: &str) -> Option<&'static str> {
    VENDORS
        .iter()
        .find(|(key, _, _, _)| *key == vendor)
        .map(|(_, _, _, package)| *package)
}

/// 벤더의 실행 파일 이름.
pub fn bin_of(vendor: &str) -> Option<&'static str> {
    VENDORS
        .iter()
        .find(|(key, _, _, _)| *key == vendor)
        .map(|(_, _, bin, _)| *bin)
}

/// blocking 조사 자체가 죽었을 때의 자리 표시 — 행은 남기고 상태만 Unknown으로.
fn unavailable(vendor: &str, label: &str, now: i64) -> VendorHealth {
    VendorHealth {
        vendor: vendor.to_string(),
        label: label.to_string(),
        auth: AuthState::Unknown,
        auth_detail: Some("상태 조사에 실패했습니다".into()),
        account: None,
        plan: None,
        installed: None,
        latest: None,
        update_available: false,
        install_method: InstallMethod::Unknown,
        bin_path: None,
        checked_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 전역 인증 캐시를 만지는 테스트를 줄 세운다.
    ///
    /// `AUTH_CACHE`는 프로세스 전역이고 `invalidate_auth_cache`는 통째로 비운다 — 병렬로 돌면
    /// 한 테스트가 넣어 둔 값을 다른 테스트가 지우고, 남은 쪽은 캐시 미스로 실제 CLI를 띄워
    /// 엉뚱한 답을 본다(이슈 #153). 캐시를 건드리는 테스트는 전부 이 락을 잡는다.
    static AUTH_CACHE_LOCK: Mutex<()> = Mutex::new(());

    fn auth_cache_guard() -> std::sync::MutexGuard<'static, ()> {
        AUTH_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn latest_cache_expires_after_ttl() {
        let now = 1_000_000;
        store_latest("test-vendor", now, Some("1.2.3".into()));
        assert_eq!(
            cached_latest("test-vendor", now + LATEST_TTL - 1),
            Some(Some("1.2.3".into()))
        );
        // TTL을 넘기면 캐시 미스 — 재조회하라는 뜻.
        assert_eq!(cached_latest("test-vendor", now + LATEST_TTL), None);
    }

    #[test]
    fn latest_cache_keeps_one_row_per_vendor() {
        let now = 2_000_000;
        store_latest("dup-vendor", now, Some("1.0.0".into()));
        store_latest("dup-vendor", now, Some("2.0.0".into()));
        let cache = LATEST_CACHE.lock().unwrap();
        assert_eq!(cache.iter().filter(|(k, _, _)| k == "dup-vendor").count(), 1);
    }

    #[test]
    fn failed_lookup_is_cached_too() {
        // 실패를 캐시하지 않으면 registry가 죽었을 때 매 렌더마다 8초를 기다린다.
        let now = 3_000_000;
        store_latest("miss-vendor", now, None);
        assert_eq!(cached_latest("miss-vendor", now + 10), Some(None));
    }

    /// 실기 확인용 — 이 머신에 실제로 깔린 CLI를 물어 결과를 찍는다.
    /// CI에는 CLI가 없으므로 기본 실행에서 뺀다: `cargo test -- --ignored real_cli`
    #[tokio::test]
    #[ignore]
    async fn real_cli_snapshot() {
        let snapshot = snapshot(true).await;
        for vendor in &snapshot.vendors {
            println!(
                "{}: auth={:?} account={:?} plan={:?} installed={:?} latest={:?} update={} method={:?} bin={:?}",
                vendor.vendor,
                vendor.auth,
                vendor.account,
                vendor.plan,
                vendor.installed,
                vendor.latest,
                vendor.update_available,
                vendor.install_method,
                vendor.bin_path,
            );
        }
        assert_eq!(snapshot.vendors.len(), 2);
    }

    /// 실기 확인용 — 조립한 액션 명령이 이 머신에서 실제로 실행 가능한 경로로 풀리는지.
    /// PTY를 띄우지는 않는다(로그인은 대화형이라 자동 검증 대상이 아니다).
    /// `cargo test -- --ignored real_cli`
    #[test]
    #[ignore]
    fn real_cli_action_commands_resolve() {
        use action::{command_for, ActionKind};
        for (vendor, _, bin, package) in VENDORS {
            let method = detect::install_method_of_bin(bin);
            for kind in [ActionKind::Login, ActionKind::Logout, ActionKind::Update] {
                match command_for(kind, vendor, method, Some(package)) {
                    Ok(command) => {
                        let resolved = detect::resolve_bin(&command.bin);
                        println!(
                            "{vendor} {kind:?}: {} {:?} → {resolved:?}",
                            command.bin, command.args
                        );
                        assert!(resolved.is_some(), "{} 를 PATH에서 못 찾았습니다", command.bin);
                    }
                    Err(reason) => println!("{vendor} {kind:?}: (없음) {reason}"),
                }
            }
        }
    }

    #[test]
    fn preflight_blocks_only_a_logged_out_known_vendor() {
        let _guard = auth_cache_guard();
        let now = now_secs();
        store_auth("claude", now, AuthState::LoggedOut);
        assert_eq!(blocking_vendor(Some("claude")).as_deref(), Some("claude"));

        // Unknown은 "확인 실패"다 — 로그아웃으로 단정해 큐를 세우면 멀쩡한 작업이 멈춘다.
        store_auth("claude", now, AuthState::Unknown);
        assert_eq!(blocking_vendor(Some("claude")), None);

        store_auth("claude", now, AuthState::Ok);
        assert_eq!(blocking_vendor(Some("claude")), None);
        invalidate_auth_cache();
    }

    #[test]
    fn preflight_ignores_agents_this_module_does_not_know() {
        // 커스텀 명령·다른 벤더는 상태를 알 수 없으니 통과시킨다. CLI를 띄우지도 않는다.
        assert_eq!(blocking_vendor(Some("my-custom-agent")), None);
        assert_eq!(blocking_vendor(Some("")), None);
        assert_eq!(blocking_vendor(None), None);
    }

    #[test]
    fn auth_cache_expires_and_is_invalidated_wholesale() {
        let _guard = auth_cache_guard();
        let now = now_secs();
        store_auth("codex", now, AuthState::LoggedOut);
        assert_eq!(cached_auth("codex", now), Some(AuthState::LoggedOut));
        assert_eq!(cached_auth("codex", now + AUTH_TTL), None);

        store_auth("codex", now, AuthState::LoggedOut);
        invalidate_auth_cache();
        // 로그인 직후 캐시가 남아 작업을 계속 막는 상황을 피하는 장치다.
        assert_eq!(cached_auth("codex", now), None);
    }

    #[test]
    fn unavailable_keeps_the_row_and_never_claims_logged_out() {
        // 조사 실패로 행이 사라지면 사용자는 벤더가 없어진 줄 안다. LoggedOut 단정도 금물.
        let health = unavailable("codex", "Codex", 42);
        assert_eq!(health.vendor, "codex");
        assert_eq!(health.auth, AuthState::Unknown);
        assert!(!health.update_available);
    }
}
