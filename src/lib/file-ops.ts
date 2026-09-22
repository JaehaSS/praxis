import type { Task } from "./ipc";

/** `a.tar.gz` → `["a", ".tar.gz"]`. 앞의 점(`.env`)은 확장자로 보지 않는다. */
const splitExt = (name: string): [string, string] => {
  const lead = name.startsWith(".") ? "." : "";
  const work = lead ? name.slice(1) : name;
  const idx = work.indexOf(".");
  return idx > 0 ? [lead + work.slice(0, idx), work.slice(idx)] : [lead + work, ""];
};

/** 같은 폴더에 복제할 때 쓸 이름. `taken`은 그 디렉터리에 이미 있는 이름들.
 *
 * 실제 이름은 백엔드 `available_name`이 정한다 — 이 함수는 같은 규칙의 프런트 사본으로,
 * 다이얼로그 기본값과 테스트에 쓴다. */
export function copyName(name: string, taken: ReadonlySet<string>): string {
  if (!taken.has(name)) return name;
  const [stem, ext] = splitExt(name);
  for (let n = 1; n < 1000; n += 1) {
    const candidate = `${stem}${n === 1 ? " 사본" : ` 사본 ${n}`}${ext}`;
    if (!taken.has(candidate)) return candidate;
  }
  return `${stem} 사본${ext}`;
}

/** 부모 디렉터리 경로. 최상위면 `/`.
 *
 * 대상 자체가 사라지거나 이름이 바뀌는 조작(휴지통·이름 변경)은 **반드시 이것**을 갱신해야
 * 한다. 폴더 자신을 갱신 대상으로 삼으면 지워진 경로를 다시 읽게 되어, 트리에서 항목이
 * 사라지지 않는다. */
export function parentPath(path: string): string {
  const at = path.lastIndexOf("/");
  if (at < 0) return "/";
  return path.slice(0, at) || "/";
}

/** 변경을 일으키는 조작 — 읽기 계열(열기·터미널·경로 복사)은 디렉터리를 건드리지 않는다. */
export type MutatingAction = "newFile" | "newDir" | "paste" | "duplicate" | "rename" | "trash";

/** 조작이 실제로 쓰는 디렉터리 — 끝난 뒤 다시 읽어야 할 곳이자 가드 판정의 기준.
 *
 * 갈림은 "무엇을 어디에 두는가" 하나다. 생성·붙여넣기는 대상 **안**에 넣으므로 폴더면 그
 * 자신이고, 이름 변경·휴지통·중복은 대상이 **사는 자리**를 건드리므로 폴더여도 부모다.
 *
 * 이 구분이 없으면 폴더에서만 셋이 한꺼번에 깨진다 — 휴지통은 지워진 경로를 다시 읽고,
 * 이름 변경은 형제가 아니라 자식과 이름 중복을 비교하며, 중복은 자기 자신 안으로 복사해
 * 백엔드에 거부당한다(`mutate.rs:74`). 파일에서는 셋 다 부모라 증상이 드러나지 않는다. */
export function actionDir(action: MutatingAction, path: string, isDir: boolean): string {
  const putsInside = action === "newFile" || action === "newDir" || action === "paste";
  return putsInside && isDir ? path : parentPath(path);
}

/** 경로가 워크트리 자신이거나 그 하위인가 — 구분자 경계에서 끊는다.
 *
 * 단순 `startsWith`면 `/a/work`가 `/a/workspace`를 삼킨다. */
const isWithin = (path: string, root: string): boolean =>
  path === root || path.startsWith(root.endsWith("/") ? root : `${root}/`);

/** 변경을 막는 활성 작업. 없으면 null.
 *
 * **표시용이다** — 메뉴를 연 뒤 작업이 시작될 수 있으므로 진실은 백엔드 재판정이다
 * (PRD §5.5). 호출부는 ACTIVE_STATES인 작업만 넘긴다. */
export function mutationBlock(
  path: string,
  tasks: readonly Task[],
): { taskId: number; reason: string } | null {
  for (const t of tasks) {
    if (t.worktree_path && isWithin(path, t.worktree_path)) {
      return { taskId: t.id, reason: `작업 #${t.id}가 이 워크트리를 쓰고 있습니다` };
    }
  }
  return null;
}

/** 이름이 쓸 수 없으면 사유, 괜찮으면 null.
 *
 * `taken`을 주면 중복도 **입력 즉시** 잡는다 — 제출 후 실패시키지 않는다(PRD §6.2 S-02).
 * 백엔드도 같은 검사를 하며 그쪽이 진실이다. */
export function validateName(name: string, taken?: ReadonlySet<string>): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "이름이 비어 있습니다";
  if (trimmed === "." || trimmed === "..") return "이름에 . 또는 .. 를 쓸 수 없습니다";
  if (/[/\\\0]/.test(trimmed)) return "이름에 / \\ 를 쓸 수 없습니다";
  if (taken?.has(trimmed)) return "같은 이름이 이미 있습니다";
  return null;
}
