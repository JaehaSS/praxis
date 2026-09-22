import type { EditorSettings } from "./ipc";

/**
 * 파일 에디터 설정 — 기본값과 **파일 트리 치수의 파생**.
 *
 * 트리는 다섯 개 치수가 한 벌로 움직여야 한다(글자·행 높이·아이콘·chevron·들여쓰기).
 * 다섯을 따로 저장하면 반드시 갈라진다 — 실제로 갈라져 있었다: `index.css`는
 * `--file-tree-chevron: 15px`라고 적어 두고 `FileTree.tsx`는 `size={14}`를 넘기고 있었다.
 * 주석이 "한 벌로 보이게 하기 위해 값을 남긴다"고 했지만, 한 벌로 **보이게** 하려던 장치가
 * 정작 거짓말을 하고 있었던 것이다.
 *
 * 그래서 사용자가 정하는 값은 **글자 크기 하나**이고 나머지 넷은 여기서 계산한다.
 * 저장소에도 글자 크기만 들어간다(`editor_tree_font_size`) — 파생값을 저장하면 그 순간
 * 다시 다섯 개의 진실이 된다.
 */

/** 기본값 = 이 설정이 생기기 전에 하드코딩돼 있던 값. 건드리지 않은 사용자는 화면이 그대로다. */
export const DEFAULT_EDITOR_SETTINGS: EditorSettings = {
  tree_font_size: 16,
  minimap: true,
  word_wrap: false,
  tab_size: 2,
};

/** 트리 글자 크기의 허용 범위 — 백엔드 clamp(10..=24)와 같은 값이어야 한다. */
export const TREE_FONT_MIN = 10;
export const TREE_FONT_MAX = 24;

export interface TreeMetrics {
  fontSize: number;
  lineHeight: number;
  icon: number;
  chevron: number;
  indent: number;
}

/**
 * 글자 크기 하나에서 트리 치수 한 벌을 뽑는다.
 *
 * 비율이 아니라 **정수 px로 확정해서** 내보낸다. 소수 px를 그대로 두면 행마다 반올림이
 * 갈려 줄 간격이 들쭉날쭉해진다(원래 `index.css` 주석이 값을 적어 둔 이유가 그것이다).
 *
 * 행 높이 1.44배는 기존 값에서 온 것이다 — 16px일 때 23px이 나와야 지금 화면과 같다.
 * 나머지 셋도 16px 입력에서 현재 CSS 값(16·15·14)을 그대로 재현한다.
 */
export function treeMetrics(fontSize: number): TreeMetrics {
  const size = clampTreeFont(fontSize);
  return {
    fontSize: size,
    lineHeight: Math.round(size * 1.44),
    icon: size,
    chevron: size - 1,
    indent: size - 2,
  };
}

/** 범위 밖 입력을 자른다. 정수가 아니면 내림 — px는 정수여야 반올림이 갈리지 않는다. */
export function clampTreeFont(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_EDITOR_SETTINGS.tree_font_size;
  return Math.min(TREE_FONT_MAX, Math.max(TREE_FONT_MIN, Math.floor(value)));
}

/**
 * 트리 치수를 CSS 변수로 내린다.
 *
 * 메인 창과 팝아웃 에디터 창이 각자 자기 `document`에 적용한다 — 두 창은 별개의 웹뷰라
 * 한쪽에 걸어도 다른 쪽에 반영되지 않는다.
 */
export function applyTreeMetrics(root: HTMLElement, fontSize: number): void {
  const m = treeMetrics(fontSize);
  root.style.setProperty("--file-tree-font-size", `${m.fontSize}px`);
  root.style.setProperty("--file-tree-line-height", `${m.lineHeight}px`);
  root.style.setProperty("--file-tree-icon", `${m.icon}px`);
  root.style.setProperty("--file-tree-chevron", `${m.chevron}px`);
  root.style.setProperty("--file-tree-indent", `${m.indent}px`);
}

/**
 * 백엔드에서 온 값을 쓸 수 있는 형태로 다듬는다.
 *
 * 필드가 빠졌거나(구버전 저장분) 범위를 벗어난 값이 와도 화면이 깨지지 않게 한다 —
 * 설정 하나가 이상해서 에디터가 통째로 못 뜨는 것이 가장 나쁜 결과다.
 */
export function normalizeEditorSettings(raw: Partial<EditorSettings> | null | undefined): EditorSettings {
  if (raw == null) return DEFAULT_EDITOR_SETTINGS;
  return {
    tree_font_size: clampTreeFont(raw.tree_font_size ?? DEFAULT_EDITOR_SETTINGS.tree_font_size),
    minimap: raw.minimap ?? DEFAULT_EDITOR_SETTINGS.minimap,
    word_wrap: raw.word_wrap ?? DEFAULT_EDITOR_SETTINGS.word_wrap,
    tab_size: Math.min(8, Math.max(1, Math.floor(raw.tab_size ?? DEFAULT_EDITOR_SETTINGS.tab_size))),
  };
}
