import type { FsNode } from "../../lib/ipc";
import { isHiddenName } from "./file-icons";

/** 펼쳐진 가지를 따라 평탄화한 행 하나. 키보드 이동은 이 배열의 인덱스로만 이야기한다. */
export interface TreeRow {
  node: FsNode;
  depth: number;
}

/** 키보드 이동에 필요한 최소 정보.
 *
 * 트리를 통째로 받는 `FileTree`와 펼친 만큼만 받아 오는 `DirectoryBrowser`는 노드 타입이
 * 다르지만, 방향키가 알아야 할 것은 "몇 번째 줄인가·폴더인가·이름이 무엇인가"뿐이다.
 * 그래서 이동 규칙은 이 모양 하나에만 의존하고 두 화면이 같은 규칙을 공유한다. */
export interface NavRow {
  path: string;
  depth: number;
  isDir: boolean;
  name: string;
}

export const toNavRows = (rows: TreeRow[]): NavRow[] =>
  rows.map(({ node, depth }) => ({
    path: node.path,
    depth,
    isDir: node.is_dir,
    name: node.name,
  }));

/** 디렉터리 우선 + 이름순. 백엔드 순서에 기대지 않고 표시 순서를 여기서 확정한다 —
 *  키보드 이동 순서와 눈에 보이는 순서가 어긋나면 방향키가 엉뚱한 곳으로 간다. */
export const sortNodes = (nodes: FsNode[]): FsNode[] =>
  [...nodes].sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

/** 화면에 실제로 그려지는 행 목록. 접힌 가지와 (숨김 해제 전) dotfile은 아예 나오지 않는다. */
export function visibleRows(
  nodes: FsNode[],
  expanded: ReadonlySet<string>,
  showHidden: boolean,
): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (list: FsNode[], depth: number) => {
    for (const node of sortNodes(list)) {
      if (!showHidden && isHiddenName(node.name)) continue;
      rows.push({ node, depth });
      if (node.is_dir && expanded.has(node.path)) walk(node.children, depth + 1);
    }
  };
  walk(nodes, 0);
  return rows;
}

/** 루트 바로 아래 디렉터리들 — 트리를 처음 열 때의 기본 펼침 상태. */
export const initialExpanded = (nodes: FsNode[]): Set<string> =>
  new Set(nodes.filter((n) => n.is_dir).map((n) => n.path));

/** 키 한 번이 트리에 요구하는 일. 렌더 없이 테스트할 수 있도록 상태 변경과 분리했다. */
export type TreeAction =
  | { kind: "focus"; index: number }
  | { kind: "expand"; path: string }
  | { kind: "collapse"; path: string }
  | { kind: "open"; path: string }
  | { kind: "toggle"; path: string }
  | { kind: "none" };

const NONE: TreeAction = { kind: "none" };

/** 같은 depth의 부모 행 인덱스 — 왼쪽 화살표가 "한 단계 밖으로" 나갈 때 쓴다. */
function parentIndex(rows: NavRow[], index: number): number {
  const depth = rows[index].depth;
  for (let i = index - 1; i >= 0; i--) {
    if (rows[i].depth < depth) return i;
  }
  return -1;
}

/**
 * 파일 트리 키 처리 — VS Code/Finder의 통상 규약을 따른다.
 *
 * 오른쪽은 "펼친다 → 이미 펼쳤으면 첫 자식으로", 왼쪽은 "접는다 → 이미 접혔으면 부모로".
 * 이 두 겹 동작이 있어야 한 손으로 깊은 트리를 오르내릴 수 있다.
 */
export function keyAction(
  key: string,
  rows: NavRow[],
  index: number,
  expanded: ReadonlySet<string>,
): TreeAction {
  if (rows.length === 0) return NONE;
  // 아직 아무 행도 잡지 않았다면 어떤 이동키든 첫 행부터 시작한다.
  if (index < 0 || index >= rows.length) {
    return key === "ArrowDown" || key === "ArrowUp" || key === "Home" || key === "End"
      ? { kind: "focus", index: key === "End" ? rows.length - 1 : 0 }
      : NONE;
  }

  const row = rows[index];
  const isDir = row.isDir;
  const open = expanded.has(row.path);

  switch (key) {
    case "ArrowDown":
      return { kind: "focus", index: Math.min(index + 1, rows.length - 1) };
    case "ArrowUp":
      return { kind: "focus", index: Math.max(index - 1, 0) };
    case "Home":
      return { kind: "focus", index: 0 };
    case "End":
      return { kind: "focus", index: rows.length - 1 };
    case "ArrowRight":
      if (!isDir) return NONE;
      if (!open) return { kind: "expand", path: row.path };
      // 펼쳐진 디렉터리의 첫 자식은 바로 다음 행이다 — 자식이 없으면 그 자리에 머문다.
      return index + 1 < rows.length && rows[index + 1].depth > row.depth
        ? { kind: "focus", index: index + 1 }
        : NONE;
    case "ArrowLeft": {
      if (isDir && open) return { kind: "collapse", path: row.path };
      const parent = parentIndex(rows, index);
      return parent >= 0 ? { kind: "focus", index: parent } : NONE;
    }
    case "Enter":
    case " ":
      return isDir ? { kind: "toggle", path: row.path } : { kind: "open", path: row.path };
    default:
      return NONE;
  }
}

/** 타이핑한 글자로 다음 행 찾기 — 현재 위치 다음부터 순환 검색한다(탐색기 관례). */
export function typeAheadIndex(rows: NavRow[], char: string, from: number): number {
  const needle = char.toLowerCase();
  for (let i = 1; i <= rows.length; i++) {
    const at = (from + i + rows.length) % rows.length;
    if (rows[at].name.toLowerCase().startsWith(needle)) return at;
  }
  return -1;
}
