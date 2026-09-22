/** base64 문자열 → Uint8Array (PTY 출력 디코드용). */
export const b64ToBytes = (b64: string): Uint8Array =>
  Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
