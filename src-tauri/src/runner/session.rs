//! 모바일 세션 — QR 페어링으로 발급하고 개별 회수 가능한 접근 자격. (설계 0013 §6)
//!
//! 원문 pairing token을 폰에 두지 않는 이유: 그 토큰은 회수 수단이 파일 교체뿐이라
//! 폰을 잃으면 Desktop 연결까지 끊어야 한다. 모바일 세션은 기기 단위로 끊을 수 있다.
//!
//! 저장은 항상 SHA-256 해시로만 한다. 토큰·코드는 32바이트 OS 난수라 사전 공격 대상이
//! 아니므로 KDF는 쓰지 않는다.

use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

/// 편집 권한이 없는 모바일 세션의 scope. (설계 0013 §6.4)
pub const SCOPE_MOBILE: &str = "mobile";

/// 세션 기본 수명. 요청이 있을 때마다 슬라이딩 갱신한다.
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;

/// 페어링 코드 수명 — QR을 띄워 스캔하는 데 필요한 만큼만.
pub const PAIRING_TTL_SECS: i64 = 5 * 60;

/// 세션 쿠키 이름. HttpOnly라 JS는 읽을 수 없다.
pub const COOKIE_NAME: &str = "praxis_mobile";

const MIGRATION: &str = "
CREATE TABLE IF NOT EXISTS mobile_pairings (
  code_hash TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  consumed_at INTEGER
);
CREATE TABLE IF NOT EXISTS mobile_sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  token_hash TEXT NOT NULL UNIQUE,
  label TEXT NOT NULL,
  scope TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  revoked_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_mobile_sessions_token ON mobile_sessions(token_hash);
";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MobileSession {
    pub id: i64,
    pub label: String,
    pub scope: String,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let mut connection = pool.acquire().await?;
    for statement in MIGRATION.split(';').filter(|s| !s.trim().is_empty()) {
        sqlx::query(statement).execute(&mut *connection).await?;
    }
    Ok(())
}

/// 32바이트 OS 난수를 hex로. 실패는 복구 대상이 아니라 즉시 오류로 올린다.
fn random_hex_32() -> anyhow::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| anyhow::anyhow!("난수 생성 실패: {error}"))?;
    Ok(hex_encode(&bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

pub fn hash_secret(secret: &str) -> String {
    hex_encode(&Sha256::digest(secret.as_bytes()))
}

/// 일회용 페어링 코드를 만들고 원문을 반환한다. 원문은 QR로만 표시하고 저장하지 않는다.
pub async fn create_pairing(pool: &SqlitePool, now: i64) -> anyhow::Result<(String, i64)> {
    let code = random_hex_32()?;
    let expires_at = now + PAIRING_TTL_SECS;
    sqlx::query(
        "INSERT INTO mobile_pairings (code_hash, created_at, expires_at, consumed_at)
         VALUES (?, ?, ?, NULL)",
    )
    .bind(hash_secret(&code))
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok((code, expires_at))
}

/// 페어링 코드를 세션으로 교환한다. 코드는 한 번만 쓸 수 있다.
/// 성공 시 세션 토큰 원문을 반환하며, 이후로는 해시만 남는다.
pub async fn redeem_pairing(
    pool: &SqlitePool,
    code: &str,
    label: &str,
    now: i64,
) -> anyhow::Result<Option<String>> {
    // 소비 표시를 조건부 UPDATE로 원자화한다 — 같은 코드의 동시 교환을 하나만 통과시킨다.
    let consumed = sqlx::query(
        "UPDATE mobile_pairings SET consumed_at = ?
         WHERE code_hash = ? AND consumed_at IS NULL AND expires_at > ?",
    )
    .bind(now)
    .bind(hash_secret(code))
    .bind(now)
    .execute(pool)
    .await?;
    if consumed.rows_affected() == 0 {
        return Ok(None);
    }

    let token = random_hex_32()?;
    let label = sanitize_label(label);
    sqlx::query(
        "INSERT INTO mobile_sessions
           (token_hash, label, scope, created_at, last_seen_at, expires_at, revoked_at)
         VALUES (?, ?, ?, ?, ?, ?, NULL)",
    )
    .bind(hash_secret(&token))
    .bind(&label)
    .bind(SCOPE_MOBILE)
    .bind(now)
    .bind(now)
    .bind(now + SESSION_TTL_SECS)
    .execute(pool)
    .await?;
    Ok(Some(token))
}

/// 기기 이름은 표시용이다. 길이를 자르고 제어문자를 제거한다.
fn sanitize_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        "모바일 기기".to_string()
    } else {
        cleaned
    }
}

/// 세션 토큰을 검증하고 마지막 접속 시각·만료를 갱신한다.
/// 만료·회수된 세션은 None. 유효하면 슬라이딩으로 수명을 연장한다.
pub async fn authenticate(
    pool: &SqlitePool,
    token: &str,
    now: i64,
) -> anyhow::Result<Option<MobileSession>> {
    let hash = hash_secret(token);
    let row = sqlx::query(
        "SELECT id, token_hash, label, scope, created_at, last_seen_at, expires_at
         FROM mobile_sessions
         WHERE token_hash = ? AND revoked_at IS NULL AND expires_at > ?",
    )
    .bind(&hash)
    .bind(now)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    // 조회는 인덱스 동등비교지만, 반환된 해시를 한 번 더 상수시간 비교해 계약을 명시한다.
    let stored: String = row.try_get("token_hash")?;
    if !crate::runner::auth::constant_time_eq(stored.as_bytes(), hash.as_bytes()) {
        return Ok(None);
    }
    let id: i64 = row.try_get("id")?;
    let expires_at = now + SESSION_TTL_SECS;
    sqlx::query("UPDATE mobile_sessions SET last_seen_at = ?, expires_at = ? WHERE id = ?")
        .bind(now)
        .bind(expires_at)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(Some(MobileSession {
        id,
        label: row.try_get("label")?,
        scope: row.try_get("scope")?,
        created_at: row.try_get("created_at")?,
        last_seen_at: now,
        expires_at,
    }))
}

pub async fn list_sessions(pool: &SqlitePool, now: i64) -> anyhow::Result<Vec<MobileSession>> {
    let rows = sqlx::query(
        "SELECT id, label, scope, created_at, last_seen_at, expires_at
         FROM mobile_sessions
         WHERE revoked_at IS NULL AND expires_at > ?
         ORDER BY last_seen_at DESC",
    )
    .bind(now)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(MobileSession {
                id: row.try_get("id")?,
                label: row.try_get("label")?,
                scope: row.try_get("scope")?,
                created_at: row.try_get("created_at")?,
                last_seen_at: row.try_get("last_seen_at")?,
                expires_at: row.try_get("expires_at")?,
            })
        })
        .collect()
}

/// 기기 하나를 즉시 끊는다. 이미 회수된 세션이면 false.
pub async fn revoke_session(pool: &SqlitePool, id: i64, now: i64) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE mobile_sessions SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// 만료된 페어링 코드를 정리한다. 세션은 만료돼도 목록 감사를 위해 남긴다.
pub async fn purge_expired_pairings(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    let result = sqlx::query("DELETE FROM mobile_pairings WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
