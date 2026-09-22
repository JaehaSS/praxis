import { useMemo } from "react";
import type { DocumentLinkTarget } from "../../lib/document-link";
import { ContextMenuShell, type MenuRow } from "./ContextMenuShell";

export type DocumentLinkMenuAction = "open" | "copyPath" | "copyLink" | "copyAbsPath" | "reveal";

export interface DocumentLinkMenuState {
  x: number;
  y: number;
  link: string;
  target: DocumentLinkTarget | null;
}

interface Props {
  menu: DocumentLinkMenuState | null;
  supportsExternalPath: boolean;
  onAction: (action: DocumentLinkMenuAction) => void;
  onClose: () => void;
}

/** 파일 미리보기 링크의 메뉴. 상대 경로와 원문 링크를 따로 복사할 수 있게 대화 메뉴와 구분한다. */
export function DocumentLinkContextMenu({ menu, supportsExternalPath, onAction, onClose }: Props) {
  const rows = useMemo<Array<MenuRow | null>>(() => {
    if (menu == null) return [];
    const item = (key: DocumentLinkMenuAction, label: string): MenuRow => ({
      key,
      label,
      onSelect: () => onAction(key),
    });
    if (menu.target?.kind === "url") return [item("open", "브라우저에서 열기"), item("copyLink", "링크 텍스트 복사")];
    if (menu.target == null) return [item("copyLink", "링크 텍스트 복사")];
    return [
      item("open", "열기"),
      item("copyPath", "경로 복사"),
      item("copyLink", "링크 텍스트 복사"),
      ...(supportsExternalPath
        ? ([item("copyAbsPath", "절대 경로 복사"), null, item("reveal", "Finder에서 보기")] as Array<MenuRow | null>)
        : []),
    ];
  }, [menu, onAction, supportsExternalPath]);

  const header = menu?.target?.kind === "file" ? menu.target.path : menu?.link ?? "";
  return (
    <ContextMenuShell
      at={menu}
      ariaLabel={menu ? `${header} 링크 조작` : ""}
      header={menu ? header : null}
      rows={rows}
      onClose={onClose}
      resetKey={menu?.link}
    />
  );
}
