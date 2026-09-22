import { useMemo } from "react";
import { ContextMenuShell, type MenuRow } from "./ContextMenuShell";

export type TabMenuAction =
  | "close"
  | "closeOthers"
  | "closeRight"
  | "copyPath"
  | "copyAbsPath"
  | "copyContent"
  | "reveal"
  | "openExternal"
  | "splitRight"
  | "splitDown";

export interface TabMenuState {
  x: number;
  y: number;
  /** 우클릭당한 탭. 활성 탭이 아닐 수도 있다 — 메뉴는 **가리킨 탭**에 대해 동작한다. */
  path: string;
}

interface Props {
  menu: TabMenuState | null;
  /** 원격 워크트리는 클라이언트 OS 경로가 없다 — Finder·기본 앱 그룹을 통째로 뺀다. */
  supportsExternalPath: boolean;
  /** 닫을 다른 탭이 있는가 / 오른쪽에 탭이 있는가. 없으면 항목을 비활성으로 둔다. */
  hasOthers: boolean;
  hasRight: boolean;
  /** 복사할 내용이 있는가. diff 탭의 본문은 스냅샷에서 그려지므로 탭 자체는 비어 있다 —
   *  빈 문자열이 소리 없이 복사되느니 항목을 빼는 편이 낫다. */
  hasContent: boolean;
  onAction: (action: TabMenuAction) => void;
  onClose: () => void;
}

const base = (p: string) => p.split("/").pop() ?? p;

/** 탭의 우클릭 메뉴. 뜨고 지는 방식은 `ContextMenuShell`이, 항목은 여기가 정한다. */
export function TabContextMenu({
  menu,
  supportsExternalPath,
  hasOthers,
  hasRight,
  hasContent,
  onAction,
  onClose,
}: Props) {
  const rows = useMemo<Array<MenuRow | null>>(() => {
    if (menu == null) return [];
    const item = (key: TabMenuAction, label: string, disabled?: boolean): MenuRow => ({
      key,
      label,
      disabled,
      onSelect: () => onAction(key),
    });
    return [
      item("close", "닫기"),
      item("closeOthers", "다른 탭 모두 닫기", !hasOthers),
      item("closeRight", "오른쪽 탭 모두 닫기", !hasRight),
      null,
      item("copyPath", "경로 복사"),
      item("copyAbsPath", "절대 경로 복사"),
      ...(hasContent ? [item("copyContent", "내용 복사")] : []),
      ...(supportsExternalPath
        ? ([null, item("reveal", "Finder에서 보기"), item("openExternal", "기본 앱으로 열기")] as Array<
            MenuRow | null
          >)
        : []),
      null,
      item("splitRight", "오른쪽으로 분할"),
      item("splitDown", "아래로 분할"),
    ];
  }, [menu, supportsExternalPath, hasOthers, hasRight, hasContent, onAction]);

  return (
    <ContextMenuShell
      at={menu}
      ariaLabel={menu ? `${base(menu.path)} 탭 조작` : ""}
      header={menu ? base(menu.path) : null}
      rows={rows}
      onClose={onClose}
      resetKey={menu?.path}
    />
  );
}
