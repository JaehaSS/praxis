/** 공용 표시 포매터 — 여러 컴포넌트에서 중복되던 것을 단일 소스로. */

/** 경과 시간(초 단위 epoch) → "3s"/"5m"/"2h"/"4d" 축약. */
export const ago = (sec: number) => {
  const d = Math.max(0, Math.floor(Date.now() / 1000) - sec);
  if (d < 60) return `${d}s`;
  if (d < 3600) return `${Math.floor(d / 60)}m`;
  if (d < 86400) return `${Math.floor(d / 3600)}h`;
  return `${Math.floor(d / 86400)}d`;
};

/** 미래 시각(epoch초)까지 남은 시간 → "3s"/"5m"/"2h"/"4d" 축약. 이미 지났으면 "0s". */
export const until = (sec: number) => {
  const d = Math.max(0, sec - Math.floor(Date.now() / 1000));
  if (d < 60) return `${d}s`;
  if (d < 3600) return `${Math.floor(d / 60)}m`;
  if (d < 86400) return `${Math.floor(d / 3600)}h`;
  return `${Math.floor(d / 86400)}d`;
};

/** 파일 크기(바이트) → "820B"/"12.4K"/"3.1M" 축약 (`ls -lh` 감각). */
export const fileSize = (bytes: number) => {
  if (bytes < 1024) return `${bytes}B`;
  const units = ["K", "M", "G", "T"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)}${units[unit]}`;
};

/** 수정 시각(epoch ms) → "01-15 15:32" (올해) / "2025-11-02" (해가 다르면). */
export const fileTime = (ms: number) => {
  if (!ms) return "";
  const d = new Date(ms);
  const p2 = (n: number) => String(n).padStart(2, "0");
  if (d.getFullYear() !== new Date().getFullYear()) {
    return `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;
  }
  return `${p2(d.getMonth() + 1)}-${p2(d.getDate())} ${p2(d.getHours())}:${p2(d.getMinutes())}`;
};

/** 시각(epoch초) → "01-15 15:32" / "2025-11-02". Task의 created_at·updated_at 단위가 초라서 필요. */
export const stamp = (sec: number) => (sec > 0 ? fileTime(sec * 1000) : "");

/** 메모리 사용 결과(outcome) → 배지 텍스트 + 색상 클래스. */
export const outcomeBadge = (o: string | null): { t: string; c: string } =>
  o === "approved"
    ? { t: "승인", c: "text-status-done" }
    : o === "discarded"
      ? { t: "버림", c: "text-status-failed" }
      : o === "success"
        ? { t: "이전 성공", c: "text-status-done" }
        : o === "failure"
          ? { t: "이전 실패", c: "text-status-failed" }
      : { t: "결과 없음", c: "text-text-muted" };
