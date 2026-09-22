/** `os-path`는 작업 worktree 밖의 클라이언트 OS 절대경로다. 일반 클릭은 검증된 읽기 전용
 *  에디터 탭으로 열고, 명시적인 OS 열기·Finder 메뉴만 기본 앱으로 넘긴다.
 *  그래서 `task-file`과 종류를 나눈다 — 호출부가 읽기 범위와 탭 권한을 구분할 수 있다. */
export type AgentLinkTarget =
  | { kind: "task-file"; path: string; line?: number; column?: number }
  | { kind: "os-path"; path: string; line?: number; column?: number }
  | { kind: "remote-file"; path: string; line?: number; column?: number }
  | { kind: "external-url"; url: string };

export interface AgentLinkOptions {
  /** 클라이언트 OS의 홈 디렉터리 절대경로. `~/…` 확장에 쓴다. 없으면 틸데 링크는 열지 않는다. */
  homePath?: string | null;
  /** 클라이언트에 실경로가 있는 호스트인가(로컬 작업만 참). 거짓이면 `os-path`를 내지 않는다. */
  externalPaths?: boolean;
  /** Runner의 허용 경로 안 파일을 읽는다. 클라이언트 OS 경로로 열지 않는다. */
  remotePaths?: boolean;
}

const URI_SCHEME = /^[a-z][a-z\d+.-]*:/i;
const WINDOWS_DRIVE = /^[a-z]:\//i;
/** `src/App.tsx:1187`·`:1187:5` — 답변과 툴 출력에서 흔한 착지 지점 표기.
 *  경로에서 떼어내야 파일을 찾을 수 있고, 뗀 값은 커서 목적지가 된다. */
const LINE_SUFFIX = /:(\d{1,7})(?::(\d{1,7}))?$/;

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function normalizedPath(value: string): string {
  const withSlashes = value.replace(/\\/g, "/");
  const withoutTrailing = withSlashes.replace(/\/+$/, "");
  return withoutTrailing || "/";
}

function fileUriPath(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.protocol !== "file:") return null;
    if (url.hostname && url.hostname !== "localhost") return null;
    return safeDecode(url.pathname);
  } catch {
    return null;
  }
}

function safeRelativePath(value: string): string | null {
  const decoded = normalizedPath(safeDecode(value)).replace(/^\.\//, "");
  const segments = decoded.split("/");
  if (!decoded || decoded === "/" || decoded.startsWith("/")) return null;
  if (segments.some((segment) => !segment || segment === "." || segment === "..")) return null;
  return decoded;
}

/** 절대경로가 루트 **안**이면 루트 기준 상대경로로, 밖이면 null. 문서 링크도 같은 판정을 쓴다. */
export function relativeToWorktree(candidate: string, worktreePath: string): string | null {
  let filePath = normalizedPath(candidate);
  const root = normalizedPath(worktreePath);
  if (WINDOWS_DRIVE.test(root) && /^\/[a-z]:\//i.test(filePath)) filePath = filePath.slice(1);

  const caseInsensitive = WINDOWS_DRIVE.test(root);
  const comparedFile = caseInsensitive ? filePath.toLowerCase() : filePath;
  const comparedRoot = caseInsensitive ? root.toLowerCase() : root;
  if (!comparedFile.startsWith(`${comparedRoot}/`)) return null;
  return safeRelativePath(filePath.slice(root.length + 1));
}

/** 홈을 붙여 절대경로로 편다. `~user/…`는 그 사용자의 홈을 알 수 없으니 대상이 아니다. */
function expandHome(link: string, homePath?: string | null): string | null {
  if (!homePath) return null;
  const home = normalizedPath(homePath);
  if (!home.startsWith("/") && !WINDOWS_DRIVE.test(home)) return null;
  if (link === "~") return home;
  // 틸데 뒤를 상대경로로 먼저 검증한다 — `~/../etc/passwd`가 홈 밖으로 나가는 것을 여기서 막는다.
  const rest = safeRelativePath(link.slice(2));
  return rest ? `${home}/${rest}` : null;
}

/** worktree 안은 상대경로 탭, 밖은 명시적으로 허용한 호스트의 읽기 전용 탭 대상이다.
 *  원격 파일의 실제 읽기 권한은 Runner가 검증하며, 옵션 없는 호출은 밖의 경로를 거부한다. */
function absoluteTarget(
  candidate: string,
  worktreePath: string,
  options: AgentLinkOptions | undefined,
  line?: number,
  column?: number,
): AgentLinkTarget | null {
  const inside = relativeToWorktree(candidate, worktreePath);
  if (inside) return asFileTarget("task-file", inside, line, column);
  if (options?.remotePaths) {
    const path = normalizedPath(candidate);
    if (/[\0-\x1f\x7f]/.test(path) || path.startsWith("//")
      || path.split("/").some((part) => part === "." || part === "..")) return null;
    return asFileTarget("remote-file", path, line, column);
  }
  if (!options?.externalPaths) return null;
  return asFileTarget("os-path", normalizedPath(candidate), line, column);
}

/** 에이전트 출력 링크를 웹 URL, 작업 내부 파일, 호스트별 외부 파일로 구분한다. */
export function resolveAgentLink(
  value: string,
  worktreePath: string,
  options?: AgentLinkOptions,
): AgentLinkTarget | null {
  const raw = value.trim();
  if (!raw || raw.includes("\0")) return null;

  // 웹 URL이 먼저다 — `https://host:8080`의 포트를 라인 번호로 읽으면 안 된다.
  if (/^https?:\/\//i.test(raw)) {
    try {
      const url = new URL(raw);
      return { kind: "external-url", url: url.href };
    } catch {
      return null;
    }
  }

  const { link, line, column } = splitLineSuffix(raw);

  if (/^file:/i.test(link)) {
    const filePath = fileUriPath(link);
    return filePath ? absoluteTarget(filePath, worktreePath, options, line, column) : null;
  }

  // 틸데는 스킴 검사보다 앞이다. 편 뒤에는 그냥 절대경로이므로 같은 규칙을 탄다 —
  // worktree 안으로 떨어지면 `os-path`가 아니라 에디터로 여는 `task-file`이 맞다.
  if (link === "~" || link.startsWith("~/")) {
    // 클라이언트 홈은 원격 홈의 근거가 아니다.
    if (options?.remotePaths) return null;
    const expanded = expandHome(link, options?.homePath);
    return expanded ? absoluteTarget(expanded, worktreePath, options, line, column) : null;
  }
  if (link.startsWith("~")) return null;

  if (URI_SCHEME.test(link)) return null;
  if (link.startsWith("/") || WINDOWS_DRIVE.test(normalizedPath(link))) {
    return absoluteTarget(safeDecode(link), worktreePath, options, line, column);
  }
  return asFileTarget("task-file", safeRelativePath(link), line, column);
}

function splitLineSuffix(value: string): { link: string; line?: number; column?: number } {
  const match = LINE_SUFFIX.exec(value);
  if (!match) return { link: value };
  return {
    link: value.slice(0, match.index),
    line: Number(match[1]),
    column: match[2] ? Number(match[2]) : undefined,
  };
}

/** 착지 지점 규칙은 종류가 달라도 한 벌이다 — line만 있으면 column은 1. */
function asFileTarget(
  kind: "task-file" | "os-path" | "remote-file",
  path: string | null,
  line?: number,
  column?: number,
): AgentLinkTarget | null {
  if (!path) return null;
  return line == null ? { kind, path } : { kind, path, line, column: column ?? 1 };
}
