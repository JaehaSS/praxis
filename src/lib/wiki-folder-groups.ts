/** 위키 그래프의 폴더별 색 배정.
 *
 *  색 슬롯이 셋뿐인 것은 취향이 아니라 검증 결과다. 노드-링크 그래프는 막대나 선과 달리
 *  임의의 두 노드가 나란히 놓이므로 인접쌍이 아니라 **전체쌍**으로 색을 판정해야 하고,
 *  네 번째 색부터는 라이트·다크 중 한쪽에서 정상 시각 분리 기준을 넘기지 못한다.
 *  그래서 문서가 많은 상위 세 폴더만 색을 갖고 나머지는 "기타"로 접는다.
 *
 *  배정은 **전체 문서**로 계산해 넘겨야 한다. 검색이나 연결 범위로 걸러진 목록으로 계산하면
 *  거르는 순간 살아남은 폴더의 색이 바뀌어, 색이 문서의 소속이 아니라 현재 필터를 뜻하게 된다. */

/** 색을 가질 수 있는 폴더 수. CSS의 `--c-cat-1..3`과 짝이다. */
export const WIKI_FOLDER_SLOTS = 3;

/** 최상위 폴더 한 칸만 본다. 하위 폴더까지 가르면 이 창고에서만 열 개가 넘어 슬롯이 모자란다. */
export function topFolder(path: string): string {
  const cut = path.indexOf("/");
  return cut < 0 ? "" : path.slice(0, cut);
}

/** 루트 문서는 폴더명이 빈 문자열이라 그대로 두면 범례에서 빈칸으로 보인다. */
export function folderLabel(folder: string): string {
  return folder || "(루트)";
}

export interface FolderGroup {
  folder: string;
  /** 0-based 색 슬롯. 색을 받지 못한 폴더는 목록에 없다. */
  slot: number;
  count: number;
}

/** 문서가 많은 폴더부터 색을 준다. 문서 수가 같으면 이름 순으로 갈라, 같은 창고에서 항상 같은 색이 나온다. */
export function folderGroups(paths: readonly string[]): FolderGroup[] {
  const counts = new Map<string, number>();
  for (const path of paths) {
    const folder = topFolder(path);
    counts.set(folder, (counts.get(folder) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort(([leftFolder, leftCount], [rightFolder, rightCount]) =>
      rightCount - leftCount || leftFolder.localeCompare(rightFolder))
    .slice(0, WIKI_FOLDER_SLOTS)
    .map(([folder, count], slot) => ({ folder, slot, count }));
}

/** 경로 → 색 슬롯. 색을 받지 못한 폴더의 문서는 `null`이며 호출자가 기타 색으로 그린다. */
export function folderSlotOf(groups: readonly FolderGroup[], path: string): number | null {
  const folder = topFolder(path);
  return groups.find(group => group.folder === folder)?.slot ?? null;
}

/** 슬롯에 대응하는 CSS 변수 이름.
 *  기타(`null`)는 중립 잉크색이다. 더 흐린 `--c-text-muted`를 쓰지 않는 이유는 그것이 링크 색이라,
 *  색이 같아지면 노드와 연결선이 한 덩어리로 보이기 때문이다. */
export function folderColorVariable(slot: number | null): string {
  return slot === null ? "--c-text-2" : `--c-cat-${slot + 1}`;
}

/** CSS에서 바로 쓸 수 있는 색 값. 테마 전환을 변수가 처리하므로 값은 여기서 계산하지 않는다. */
export function folderColor(slot: number | null): string {
  return `var(${folderColorVariable(slot)})`;
}

/** 기타 그룹의 범례 이름. 슬롯을 받지 못한 폴더 전부를 가리킨다. */
export const OTHER_FOLDER_LABEL = "기타";
