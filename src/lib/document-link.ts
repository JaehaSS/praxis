import { relativeToWorktree } from "./agent-link";

export type DocumentLinkTarget =
  | { kind: "file"; path: string }
  | { kind: "url"; url: string };

const URI_SCHEME = /^[a-z][a-z\d+.-]*:/i;
const WINDOWS_DRIVE = /^[a-z]:\//i;
const LINE_SUFFIX = /:\d{1,7}(?::\d{1,7})?$/;

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function relativePath(link: string, sourcePath: string, rootPath?: string | null): string | null {
  const withoutFragment = link.split("#", 1)[0];
  const decoded = safeDecode(withoutFragment.replace(LINE_SUFFIX, "")).replace(/\\/g, "/");
  if (!decoded || /[\0-\x1f\x7f]/.test(decoded)) return null;
  // 절대 경로는 문서 위치와 무관하다 — 루트 안이냐만 따진다.
  if (decoded.startsWith("/") || WINDOWS_DRIVE.test(decoded)) {
    return rootPath ? relativeToWorktree(decoded, rootPath) : null;
  }
  if (decoded.startsWith("~") || URI_SCHEME.test(decoded)) return null;

  const parts = sourcePath.split("/").slice(0, -1);
  if (sourcePath.startsWith("/") || sourcePath.includes("\\") || parts.some((part) => !part || part === "." || part === "..")) return null;
  for (const part of decoded.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      if (parts.length === 0) return null;
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  return parts.join("/") || null;
}

/** Markdown 문서 링크는 브라우저 출처가 아니라 링크가 적힌 문서의 디렉터리를 기준으로 푼다.
 *  루트 안 절대 경로는 루트 기준 상대 경로로 푼다 — 루트 밖은 연다고 약속하지 않는다. */
export function resolveDocumentLink(
  value: string,
  sourcePath: string,
  rootPath?: string | null,
): DocumentLinkTarget | null {
  const link = value.trim();
  if (!link || link.includes("\0")) return null;
  if (/^https?:\/\//i.test(link)) {
    try {
      return { kind: "url", url: new URL(link).href };
    } catch {
      return null;
    }
  }
  const path = relativePath(link, sourcePath, rootPath);
  return path ? { kind: "file", path } : null;
}
