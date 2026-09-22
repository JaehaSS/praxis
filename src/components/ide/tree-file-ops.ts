import type { FsNode } from "../../lib/ipc";

/**
 * 파일 트리에서 무언가를 만들 때의 자리 계산 — 순수 함수.
 *
 * 경로는 전부 **워크트리 루트 기준 상대 경로**다(`FsNode.path`). 루트 자신은 빈 문자열이며,
 * 백엔드의 `safe_join(root, "")`이 루트를 그대로 돌려주므로 그대로 넘길 수 있다.
 */

/** 짚은 노드 기준으로 새 항목이 생길 디렉터리. 디렉터리면 그 안, 파일이면 그 옆(부모). */
export function targetDir(node: { path: string; is_dir: boolean } | null): string {
  if (node == null) return "";
  if (node.is_dir) return node.path;
  const at = node.path.lastIndexOf("/");
  return at < 0 ? "" : node.path.slice(0, at);
}

/** 상대 경로 이어붙이기. 루트(빈 문자열)에 붙일 때 앞에 슬래시가 남지 않게 한다. */
export function joinPath(dir: string, name: string): string {
  return dir === "" ? name : `${dir}/${name}`;
}

/**
 * 그 디렉터리에 이미 있는 이름들 — 다이얼로그가 제출 전에 중복을 잡는 데 쓴다.
 *
 * 트리에서 찾지 못하면 빈 집합을 준다. 검사가 느슨해질 뿐이고 최종 판정은 백엔드가 하므로
 * (거부 사유는 다이얼로그에 인라인으로 뜬다) 여기서 없는 경로를 만들어 내지 않는다.
 */
export function namesIn(nodes: FsNode[], dir: string): Set<string> {
  let level = nodes;
  if (dir !== "") {
    for (const part of dir.split("/")) {
      const found = level.find((n) => n.is_dir && n.name === part);
      if (found == null) return new Set();
      level = found.children;
    }
  }
  return new Set(level.map((n) => n.name));
}

/**
 * 이름이 바뀐 자리를 따라 경로를 고쳐 준다. 해당 없으면 null.
 *
 * 디렉터리를 바꾸면 그 아래 **열려 있던 탭이 전부** 옛 경로를 물게 된다. 닫아 버리면
 * "폴더 이름을 바꿨더니 보던 파일이 다 사라졌다"가 되므로, 같은 파일을 새 경로로 다시 연다.
 */
export function rewritePath(path: string, from: string, to: string): string | null {
  if (path === from) return to;
  if (path.startsWith(`${from}/`)) return to + path.slice(from.length);
  return null;
}

/** 이 경로를 건드리면 함께 영향받는 열린 탭들 — 자신과 그 하위 전부. */
export function affectedPaths(openPaths: readonly string[], target: string): string[] {
  return openPaths.filter((p) => p === target || p.startsWith(`${target}/`));
}

/** 이 항목이 든 디렉터리. 루트 바로 아래면 빈 문자열이다. */
export function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** 이름을 바꾼 뒤의 경로 — 부모는 그대로 두고 마지막 마디만 갈아 끼운다. */
export function renamedPath(path: string, name: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? name : `${path.slice(0, at)}/${name}`;
}

