//! 설정 > 모바일 — 표면 서빙 토글·포트·잠자기 방지·페어링·세션 관리 (설계 2026-09-13 P2).
//!
//! 페어링 코드와 세션 목록은 러너와 같은 `runner::session`을 쓴다. 데스크톱 DB에
//! `mobile_pairings`·`mobile_sessions` 테이블이 있는 것이 전제다(`mobile_surface::migrate`).

use tauri::{AppHandle, Manager, State};

use super::{pool_of, AppState};
use crate::mobile_surface::{self, MobileSurface, MobileSurfaceStatus};
use crate::runner::session::{self, MobileSession};

#[tauri::command]
pub async fn mobile_surface_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<MobileSurfaceStatus, String> {
    let pool = pool_of(&state)?;
    let prevent_sleep = mobile_surface::prevent_sleep_enabled(&pool).await;
    let port = mobile_surface::configured_port(&pool).await;
    Ok(app.state::<MobileSurface>().status(prevent_sleep, port))
}

/// 서빙 on/off. 설정에 먼저 기록하고 실제 기동한다 — 다음 부팅에서 되살아나야 한다.
#[tauri::command]
pub async fn mobile_surface_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<MobileSurfaceStatus, String> {
    let pool = pool_of(&state)?;
    crate::db::set_setting(
        &pool,
        mobile_surface::ENABLED_KEY,
        if enabled { "true" } else { "false" },
    )
    .await
    .map_err(|error| error.to_string())?;
    if enabled {
        mobile_surface::start(&app, &pool).await
    } else {
        app.state::<MobileSurface>().stop();
        let prevent_sleep = mobile_surface::prevent_sleep_enabled(&pool).await;
        let port = mobile_surface::configured_port(&pool).await;
        Ok(app.state::<MobileSurface>().status(prevent_sleep, port))
    }
}

/// 포트 변경. 떠 있으면 새 포트로 다시 띄운다 — 설정만 바뀌고 실물이 그대로면 거짓말이 된다.
#[tauri::command]
pub async fn mobile_surface_set_port(
    app: AppHandle,
    state: State<'_, AppState>,
    port: u16,
) -> Result<MobileSurfaceStatus, String> {
    if port < 1024 {
        return Err("1024 이상의 포트를 쓰세요".into());
    }
    let pool = pool_of(&state)?;
    crate::db::set_setting(&pool, mobile_surface::PORT_KEY, &port.to_string())
        .await
        .map_err(|error| error.to_string())?;
    let surface = app.state::<MobileSurface>();
    if surface.is_running() {
        surface.stop();
        return mobile_surface::start(&app, &pool).await;
    }
    let prevent_sleep = mobile_surface::prevent_sleep_enabled(&pool).await;
    Ok(surface.status(prevent_sleep, port))
}

/// 잠자기 방지 토글. `caffeinate` 자식 프로세스의 수명이 서버와 묶여 있어 재기동한다.
#[tauri::command]
pub async fn mobile_surface_set_prevent_sleep(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<MobileSurfaceStatus, String> {
    let pool = pool_of(&state)?;
    crate::db::set_setting(
        &pool,
        mobile_surface::PREVENT_SLEEP_KEY,
        if enabled { "true" } else { "false" },
    )
    .await
    .map_err(|error| error.to_string())?;
    let surface = app.state::<MobileSurface>();
    if surface.is_running() {
        surface.stop();
        return mobile_surface::start(&app, &pool).await;
    }
    let port = mobile_surface::configured_port(&pool).await;
    Ok(surface.status(enabled, port))
}

/// 일회용 페어링 코드. 원문은 이 응답에만 존재한다 — DB에는 해시만 남는다.
#[tauri::command]
pub async fn mobile_pairing_create(
    state: State<'_, AppState>,
) -> Result<crate::runner::http::PairingResponse, String> {
    let pool = pool_of(&state)?;
    mobile_surface::migrate(&pool)
        .await
        .map_err(|error| error.to_string())?;
    let (code, expires_at) = session::create_pairing(&pool, crate::now())
        .await
        .map_err(|error| error.to_string())?;
    Ok(crate::runner::http::PairingResponse { code, expires_at })
}

#[tauri::command]
pub async fn mobile_session_list(state: State<'_, AppState>) -> Result<Vec<MobileSession>, String> {
    let pool = pool_of(&state)?;
    mobile_surface::migrate(&pool)
        .await
        .map_err(|error| error.to_string())?;
    session::list_sessions(&pool, crate::now())
        .await
        .map_err(|error| error.to_string())
}

/// 세션 해지 — 그 기기의 푸시 구독도 함께 지운다. 자격이 없는 기기에 알림이 계속 가면 안 된다.
#[tauri::command]
pub async fn mobile_session_revoke(state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    let pool = pool_of(&state)?;
    let revoked = session::revoke_session(&pool, id, crate::now())
        .await
        .map_err(|error| error.to_string())?;
    if revoked {
        let _ = crate::runner::push::remove_for_session(&pool, id).await;
    }
    Ok(revoked)
}
