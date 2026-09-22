import type { FsNode } from "../lib/ipc";

// worktree 트리 탐색 — 순수 로직. (설계 0013 §10 M3)
//
// 데스크톱은 재귀 트리를 통째로 들여쓰기해 보여주지만, 폰에서는 3단만 들어가도 가로가
// 남지 않는다. 여기서는 **한 단계씩 들어가는 드릴다운**으로 바꾼다 — 백엔드가 주는
// 중첩 트리를 그대로 쓰되, 현재 디렉터리의 자식만 꺼내 보여준다.

/** 루트를 뜻하는 경로. 백엔드 FsNode.path는 worktree 루트 기준 상대경로다. */
export const ROOT = "";

/** 경로에 해당하는 노드를 찾는다. 루트("")는 노드가 없으므로 null. */
export function findNode(nodes: FsNode[], path: string): FsNode | null {
  if (path === ROOT) return null;
  for (const node of nodes) {
    if (node.path === path) return node;
    // 경로 접두사가 일치할 때만 내려간다 — 형제 서브트리를 전부 훑지 않는다.
    if (node.is_dir && path.startsWith(`${node.path}/`)) {
      const found = findNode(node.children, path);
      if (found) return found;
    }
  }
  return null;
}

/** 디렉터리 먼저, 그다음 이름순. 대소문자를 섞어 정렬하면 눈으로 훑기 어렵다. */
function ordered(nodes: FsNode[]): FsNode[] {
  return [...nodes].sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    return a.name.localeCompare(b.name, "en", { sensitivity: "base" });
  });
}

/** 해당 경로에서 보여줄 항목들. 파일을 가리키거나 없는 경로면 빈 배열. */
export function childrenAt(nodes: FsNode[], path: string): FsNode[] {
  if (path === ROOT) return ordered(nodes);
  const node = findNode(nodes, path);
  return node?.is_dir ? ordered(node.children) : [];
}

/** 한 단계 위 경로. 루트에서는 그대로 루트. */
export function parentPath(path: string): string {
  const index = path.lastIndexOf("/");
  return index < 0 ? ROOT : path.slice(0, index);
}

export interface Crumb {
  name: string;
  path: string;
}

/** 상단 경로 표시용. 첫 항목은 항상 루트다. */
export function breadcrumbs(path: string): Crumb[] {
  const crumbs: Crumb[] = [{ name: "/", path: ROOT }];
  if (path === ROOT) return crumbs;
  let accumulated = "";
  for (const segment of path.split("/").filter(Boolean)) {
    accumulated = accumulated ? `${accumulated}/${segment}` : segment;
    crumbs.push({ name: segment, path: accumulated });
  }
  return crumbs;
}

/** 미리보기 불가 종류에 대해 이유를 말한다. 빈 화면으로 두면 고장으로 보인다. */
export function previewNotice(kind: string): string | null {
  switch (kind) {
    case "binary":
      return "이 파일은 텍스트가 아니라 폰에서 열 수 없습니다.";
    case "too_large":
      return "파일이 너무 커서 미리보기를 만들지 않았습니다.";
    case "table":
      return "표 파일(parquet)은 데스크톱 에디터에서 미리 볼 수 있습니다.";
    default:
      return null;
  }
}
