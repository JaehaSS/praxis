import { useEffect, useRef, useState } from "react";
import { editorSettingsSet, type EditorSettings } from "../../lib/ipc";
import {
  TREE_FONT_MAX,
  TREE_FONT_MIN,
  applyTreeMetrics,
  clampTreeFont,
  treeMetrics,
} from "../../lib/editor-settings";
import { Icon } from "./icons";
import { useHighlight } from "./settings/SettingRow";

interface Props {
  settings: EditorSettings;
  /** 저장이 끝난 값 — App이 받아 CSS 변수와 Monaco 옵션에 반영한다. */
  onChange: (next: EditorSettings) => void;
}

const row = "flex items-center justify-between gap-4 py-3 border-b border-border";

/** 설정 패널의 다른 스위치와 같은 모양 — SettingsPanel의 `sw`와 시각적으로 동일하다. */
function Switch({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      role="switch"
      aria-checked={on}
      className={`w-11 h-6 shrink-0 rounded-full relative transition-colors ${on ? "bg-primary" : "bg-border"}`}
    >
      <span
        className={`absolute top-0.5 w-5 h-5 rounded-full bg-bg transition-all ${on ? "left-[22px]" : "left-0.5"}`}
      />
    </button>
  );
}

/**
 * 에디터 설정 — 파일 트리 치수와 Monaco 동작.
 *
 * 트리 크기만 저장 시점이 다르다. 슬라이더를 끄는 **동안에는 CSS 변수만** 움직여 즉시 보이게
 * 하고, 저장(IPC)과 부모 state 갱신은 손을 뗄 때 한 번 한다 — 매 픽셀마다 DB를 때리면
 * 드래그 한 번에 수십 번 쓰게 된다. 토글·탭 크기는 값이 띄엄띄엄해서 즉시 커밋한다.
 */
export function EditorSettingsTab({ settings, onChange }: Props) {
  const [treeSize, setTreeSize] = useState(settings.tree_font_size);
  const [error, setError] = useState<string | null>(null);
  /** 저장에 실패했을 때 되돌아갈 값 — 화면과 저장분이 갈리지 않게 한다. */
  const savedRef = useRef(settings.tree_font_size);

  // 밖에서 값이 바뀌면(부팅 로드·다른 창) 슬라이더도 따라간다.
  useEffect(() => {
    setTreeSize(settings.tree_font_size);
    savedRef.current = settings.tree_font_size;
  }, [settings.tree_font_size]);

  const preview = (size: number) => {
    applyTreeMetrics(document.documentElement, size);
  };

  const commit = (next: EditorSettings) => {
    setError(null);
    editorSettingsSet(next)
      .then(() => {
        savedRef.current = next.tree_font_size;
        onChange(next);
      })
      .catch((e) => {
        setError(String(e));
        // 저장이 안 됐는데 화면만 새 값이면 다음 부팅에 되돌아간다 — 지금 되돌린다.
        setTreeSize(savedRef.current);
        preview(savedRef.current);
      });
  };

  const commitTreeSize = () => {
    const size = clampTreeFont(treeSize);
    if (size === savedRef.current) return;
    commit({ ...settings, tree_font_size: size });
  };

  const m = treeMetrics(treeSize);
  // 검색이 지목하면 해당 섹션이 스스로 스크롤·강조한다 — 패널이 DOM을 뒤지지 않는다.
  const tree = useHighlight("editor-tree");
  const code = useHighlight("editor-code");

  return (
    <div className="flex-1 overflow-auto p-6">
      <div className="max-w-3xl mx-auto">
        <h2 className="text-lg font-semibold mb-4">에디터</h2>

        <section ref={tree.ref} data-setting-id="editor-tree" className={`pb-2 rounded-md ${tree.ring}`}>
          <div className="text-text font-medium">파일 트리</div>
          <div className="text-text-muted text-xs mb-2">
            글자 크기 하나로 행 높이·아이콘·들여쓰기가 함께 움직입니다. 글자만 키우면 행 높이가
            따라오지 않아 g·y·p의 아래가 잘립니다.
          </div>

          <div className={row}>
            <div className="min-w-0">
              <div className="text-text">글자 크기</div>
              <div className="text-text-muted text-xs">
                {m.fontSize}px · 행 {m.lineHeight}px · 아이콘 {m.icon}px
              </div>
            </div>
            <div className="flex items-center gap-3 shrink-0">
              <input
                type="range"
                min={TREE_FONT_MIN}
                max={TREE_FONT_MAX}
                step={1}
                value={treeSize}
                aria-label="파일 트리 글자 크기"
                className="w-40 accent-primary"
                onChange={(e) => {
                  const n = Number(e.target.value);
                  setTreeSize(n);
                  preview(n); // 끄는 동안에는 CSS만 — 저장은 손을 뗄 때.
                }}
                onPointerUp={commitTreeSize}
                onKeyUp={commitTreeSize}
                onBlur={commitTreeSize}
              />
              <input
                type="number"
                min={TREE_FONT_MIN}
                max={TREE_FONT_MAX}
                aria-label="파일 트리 글자 크기(px)"
                className="w-16 bg-bg border border-border rounded px-2 py-1 text-sm text-text outline-none focus:border-primary"
                value={treeSize}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isNaN(n)) return;
                  setTreeSize(n);
                  preview(clampTreeFont(n));
                }}
                onBlur={commitTreeSize}
                onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
              />
            </div>
          </div>

          {/* 설정 화면에서는 트리가 안 보일 수 있다 — 값을 고르는 자리에서 결과가 보여야 한다. */}
          <div
            data-file-tree
            className="mt-3 rounded border border-border bg-raised p-2 select-none"
            style={{ fontSize: `${m.fontSize}px`, lineHeight: `${m.lineHeight}px` }}
          >
            {[
              { name: "src", dir: true, depth: 0 },
              { name: "EditorPane.tsx", dir: false, depth: 1 },
              { name: "README.md", dir: false, depth: 0 },
            ].map((n) => (
              <div
                key={n.name}
                className="flex items-center gap-1"
                style={{ paddingLeft: `${n.depth * m.indent}px` }}
              >
                <span
                  className="ft-chevron shrink-0 text-text-muted"
                  style={{ width: `${m.chevron}px` }}
                >
                  {n.dir && <Icon name="chevronDown" size={14} />}
                </span>
                <span className="ft-icon shrink-0 flex items-center text-text-muted">
                  <Icon name={n.dir ? "folder" : "file"} size={16} />
                </span>
                <span className={n.dir ? "font-medium" : ""}>{n.name}</span>
              </div>
            ))}
          </div>
        </section>

        <section ref={code.ref} data-setting-id="editor-code" className={`pt-4 rounded-md ${code.ring}`}>
          <div className="text-text font-medium">코드 편집</div>
          <div className="text-text-muted text-xs mb-2">
            열려 있는 편집기에 바로 적용됩니다.
          </div>

          <div className={row}>
            <div className="min-w-0">
              <div className="text-text">미니맵</div>
              <div className="text-text-muted text-xs">
                오른쪽 축소 지도. 화면을 세로로 나눠 쓸 때는 꺼 두면 폭이 넓어집니다.
              </div>
            </div>
            <Switch
              on={settings.minimap}
              onClick={() => commit({ ...settings, minimap: !settings.minimap })}
            />
          </div>

          <div className={row}>
            <div className="min-w-0">
              <div className="text-text">줄 바꿈</div>
              <div className="text-text-muted text-xs">
                긴 줄을 창 너비에서 접습니다. 끄면 가로로 스크롤합니다.
              </div>
            </div>
            <Switch
              on={settings.word_wrap}
              onClick={() => commit({ ...settings, word_wrap: !settings.word_wrap })}
            />
          </div>

          <div className={row}>
            <div className="min-w-0">
              <div className="text-text">탭 크기</div>
              <div className="text-text-muted text-xs">들여쓰기 한 단계의 칸 수.</div>
            </div>
            <input
              type="number"
              min={1}
              max={8}
              aria-label="탭 크기"
              className="w-16 shrink-0 bg-bg border border-border rounded px-2 py-1 text-sm text-text outline-none focus:border-primary"
              value={settings.tab_size}
              onChange={(e) => {
                const n = Number(e.target.value);
                if (Number.isNaN(n)) return;
                commit({ ...settings, tab_size: Math.min(8, Math.max(1, Math.floor(n))) });
              }}
            />
          </div>
        </section>

        {error && <div className="mt-3 text-status-failed text-sm">설정 저장 실패: {error}</div>}
      </div>
    </div>
  );
}
