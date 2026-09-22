//! 채널 시크릿(텔레그램 봇 토큰 등) OS 키체인 저장 — 평문 SQLite(`channel_secrets`) 대체.
//!
//! keyring-core는 동기 API라 각 함수는 `tauri::async_runtime::spawn_blocking`으로 감싸
//! 호출부(async 컨텍스트)와의 시그니처를 맞춘다. 평문 폴백 없음 — 키체인 접근 실패는
//! 항상 Err로 표면화한다(조용히 SQLite로 떨어지면 이관 의미가 없음).
//!
//! 테스트 격리: 이 모듈은 스토어 선택(`ensure_store`)과 keyring 직접 호출부만 담당하고,
//! 실제 스토어 초기화는 `#[cfg(not(test))]`로 프로덕션 빌드에만 적용된다. `cargo test`는
//! `test_support::install_mock_store()`가 `keyring_core::mock::Store`를 등록하므로 실제
//! OS 키체인에 전혀 접근하지 않는다(프롬프트 없음).

use std::{
    collections::HashMap,
    sync::{Mutex, Once, OnceLock},
};

use keyring_core::Entry;

/// 키체인 엔트리의 service 이름 — kind(예: "telegram")가 account.
const SERVICE: &str = "dev.praxis.channel";

static INIT_STORE: Once = Once::new();
type CachedSecret = Result<Option<String>, String>;
static SECRET_CACHE: OnceLock<Mutex<HashMap<String, CachedSecret>>> = OnceLock::new();

fn secret_cache() -> &'static Mutex<HashMap<String, CachedSecret>> {
    SECRET_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 기본 크리덴셜 스토어 준비(1회) — 프로덕션은 macOS Keychain, 테스트는 mock(별도 초기화).
fn ensure_store() {
    INIT_STORE.call_once(|| {
        #[cfg(not(test))]
        install_platform_store();
        #[cfg(test)]
        test_support::install_mock_store();
    });
}

#[cfg(not(test))]
fn install_platform_store() {
    #[cfg(target_os = "macos")]
    {
        match apple_native_keyring_store::keychain::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(e) => eprintln!("키체인 스토어 초기화 실패: {e}"),
        }
    }
    #[cfg(target_os = "windows")]
    {
        match windows_native_keyring_store::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(e) => eprintln!("Windows Credential Manager 초기화 실패: {e}"),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        eprintln!("이 플랫폼용 키체인 스토어 미지원 — 시크릿 저장이 실패할 수 있음");
    }
}

fn entry(kind: &str) -> Result<Entry, String> {
    ensure_store();
    Entry::new(SERVICE, kind).map_err(|e| format!("키체인 엔트리 생성 실패: {e}"))
}

/// 시크릿 저장(upsert). 실패는 Err로 표면화 — 평문 폴백 없음.
pub async fn set_secret(kind: &str, value: &str) -> Result<(), String> {
    let kind = kind.to_string();
    let value = value.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut cache = secret_cache().lock().unwrap_or_else(|e| e.into_inner());
        let result = entry(&kind)?
            .set_password(&value)
            .map_err(|e| format!("키체인 저장 실패: {e}"));
        cache.insert(kind, result.clone().map(|()| Some(value)));
        result
    })
    .await
    .map_err(|e| format!("키체인 작업 실행 실패: {e}"))?
}

/// 시크릿 조회. 프로세스에서 첫 조회 결과(없음/접근 오류 포함)를 캐시해 macOS Keychain
/// 승인 UI가 폴링마다 반복되지 않게 한다. 저장/삭제는 캐시를 즉시 갱신한다.
pub async fn get_secret(kind: &str) -> Result<Option<String>, String> {
    let kind = kind.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut cache = secret_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = cache.get(&kind) {
            return cached.clone();
        }
        let result = match entry(&kind)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("키체인 조회 실패: {e}")),
        };
        cache.insert(kind, result.clone());
        result
    })
    .await
    .map_err(|e| format!("키체인 작업 실행 실패: {e}"))?
}

/// 시크릿 삭제. 없어도 Ok(()) (idempotent).
pub async fn clear_secret(kind: &str) -> Result<(), String> {
    let kind = kind.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut cache = secret_cache().lock().unwrap_or_else(|e| e.into_inner());
        let result = match entry(&kind)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("키체인 삭제 실패: {e}")),
        };
        cache.insert(kind, result.clone().map(|()| None));
        result
    })
    .await
    .map_err(|e| format!("키체인 작업 실행 실패: {e}"))?
}

/// 테스트 전용 — mock 크리덴셜 스토어 설치. 실제 OS 키체인에 절대 접근하지 않는다.
#[cfg(test)]
mod test_support {
    pub fn install_mock_store() {
        let store = keyring_core::mock::Store::new().expect("mock store 생성 실패");
        keyring_core::set_default_store(store);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 모든 테스트가 동일 프로세스 내 static Once를 공유하므로, 첫 테스트에서 mock이 설치되면
    // 이후 테스트도 동일 mock 스토어(프로세스 전역, in-memory)를 재사용한다 — 실제 키체인 미접근.

    #[tokio::test]
    async fn set_get_roundtrip() {
        let kind = "test_roundtrip";
        clear_secret(kind).await.ok();
        set_secret(kind, "shh").await.unwrap();
        assert_eq!(get_secret(kind).await.unwrap(), Some("shh".to_string()));
        clear_secret(kind).await.unwrap();
        assert_eq!(get_secret(kind).await.unwrap(), None);
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        assert_eq!(get_secret("test_never_set").await.unwrap(), None);
    }

    #[tokio::test]
    async fn clear_missing_is_ok() {
        clear_secret("test_never_set_either").await.unwrap();
    }

    #[tokio::test]
    async fn set_overwrites_existing() {
        let kind = "test_overwrite";
        set_secret(kind, "first").await.unwrap();
        set_secret(kind, "second").await.unwrap();
        assert_eq!(get_secret(kind).await.unwrap(), Some("second".to_string()));
        clear_secret(kind).await.unwrap();
    }

    #[tokio::test]
    async fn repeated_get_uses_process_cache() {
        let kind = "test_cached_read";
        let direct = entry(kind).unwrap();
        direct.delete_credential().ok();
        direct.set_password("first").unwrap();

        assert_eq!(get_secret(kind).await.unwrap(), Some("first".to_string()));
        direct.set_password("changed_outside_praxis").unwrap();
        assert_eq!(get_secret(kind).await.unwrap(), Some("first".to_string()));

        clear_secret(kind).await.unwrap();
    }
}
