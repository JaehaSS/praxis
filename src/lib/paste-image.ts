/** 클립보드 이미지 붙여넣기 — DataTransfer에서 이미지 추출, base64 인코딩, 프롬프트 삽입
 *  텍스트 생성. 파일 저장은 ipc `pasteImageSave`(백엔드)가 담당하고, 여기서는 붙여넣기
 *  이벤트 처리에 필요한 순수 로직만 둔다. */

/** DataTransfer에서 첫 이미지 파일을 찾는다 — 없으면 null(기본 텍스트 붙여넣기 유지). */
export function findImageFile(data: DataTransfer | null): File | null {
  if (!data) return null;
  for (const item of Array.from(data.items)) {
    if (item.kind === "file" && item.type.startsWith("image/")) {
      const file = item.getAsFile();
      if (file) return file;
    }
  }
  return null;
}

/** 프롬프트에 삽입할 참조 텍스트 — Design Mode 캡처의 `이미지: <경로>` 주입과 같은 전달
 *  방식(에이전트 CLI가 경로의 파일을 직접 읽는다). */
export function pastedImageRef(path: string): string {
  return `[이미지: ${path}]`;
}

/** caret 위치에 텍스트 삽입 — 인접 문자가 공백이 아니면 한 칸 띄워 경로가 단어에 붙지 않게.
 *  반환 caret은 삽입분(뒤 공백 포함) 바로 뒤. */
export function insertAtCaret(
  value: string,
  caret: number,
  text: string,
): { value: string; caret: number } {
  const at = Math.max(0, Math.min(caret, value.length));
  const before = value.slice(0, at);
  const after = value.slice(at);
  const lead = before && !/\s$/.test(before) ? " " : "";
  const trail = after && !/^\s/.test(after) ? " " : "";
  const inserted = `${lead}${text}${trail}`;
  return { value: before + inserted + after, caret: at + inserted.length };
}

/** Blob → base64 문자열. FileReader 없이 구현해 브라우저·테스트(node) 양쪽에서 동작한다. */
export async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  const CHUNK = 0x8000; // String.fromCharCode 인자 수 제한(콜스택) 회피
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}
