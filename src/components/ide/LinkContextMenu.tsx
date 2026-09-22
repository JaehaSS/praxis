import { useMemo } from "react";
import { ContextMenuShell, type MenuRow } from "./ContextMenuShell";

export type LinkMenuAction = "open" | "copyLink" | "copyAbsPath" | "reveal" | "openExternal";

/** 링크가 실제로 무엇을 가리키는지. 해석 결과에 따라 항목이 달라진다. */
export type LinkMenuKind = "file" | "url" | "unresolved";

export interface LinkMenuState {
  x: number;
  y: number;
  /** 우클릭당한 링크의 원문(href). 복사·열기 모두 이 값을 기준으로 한다. */
  link: string;
}

interface Props {
  menu: LinkMenuState | null;
  kind: LinkMenuKind;
  /** 클라이언트 OS에 실경로가 있는 호스트인가(로컬 작업만 참). 원격은 Finder·기본 앱 그룹을 통째로 뺀다. */
  supportsExternalPath: boolean;
  onAction: (action: LinkMenuAction) => void;
  onClose: () => void;
}

/**
 * 머리말에 쓸 표시명. 파일은 마지막 세그먼트가 곧 파일명이지만, url은 그것이 비거나
 * (끝 슬래시) 쿼리 조각만 남아 어디인지 알 수 없다 — url은 원문을 그대로 둔다.
 */
const display = (kind: LinkMenuKind, link: string): string =>
  kind === "url" ? link : (link.split("/").pop() ?? link);

/** 대화 본문에 렌더된 링크의 우클릭 메뉴. 뜨고 지는 방식은 `ContextMenuShell`이, 항목은 여기가 정한다. */
export function LinkContextMenu({ menu, kind, supportsExternalPath, onAction, onClose }: Props) {
  const rows = useMemo<Array<MenuRow | null>>(() => {
    if (menu == null) return [];
    const item = (key: LinkMenuAction, label: string): MenuRow => ({
      key,
      label,
      onSelect: () => onAction(key),
    });
    if (kind === "url") return [item("open", "브라우저에서 열기"), item("copyLink", "링크 복사")];
    // 해석에 실패한 링크는 열 대상이 없다 — 원문을 넘겨줄 복사 하나만 남긴다.
    if (kind === "unresolved") return [item("copyLink", "링크 텍스트 복사")];
    return [
      item("open", "열기"),
      null,
      item("copyLink", "링크 텍스트 복사"),
      // 절대 경로도 Finder·기본 앱도 클라이언트 OS의 실경로를 전제한다. 원격이면 통째로 뺀다.
      ...(supportsExternalPath
        ? ([
            item("copyAbsPath", "절대 경로 복사"),
            null,
            item("reveal", "Finder에서 보기"),
            item("openExternal", "기본 앱으로 열기"),
          ] as Array<MenuRow | null>)
        : []),
    ];
  }, [menu, kind, supportsExternalPath, onAction]);

  const name = menu ? display(kind, menu.link) : "";

  return (
    <ContextMenuShell
      at={menu}
      ariaLabel={menu ? `${name} 링크 조작` : ""}
      header={menu ? name : null}
      rows={rows}
      onClose={onClose}
      resetKey={menu?.link}
    />
  );
}
