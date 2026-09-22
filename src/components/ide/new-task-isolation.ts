export type TaskRuntime = "local" | "remote";

/**
 * 격리가 강제인가 — 원격 작업과 앙상블(후보 여러 개가 같은 repo를 건드림)은
 * 백엔드가 설정과 무관하게 항상 워크트리를 만든다(`create_task_internal`).
 * 강제인 곳의 칩은 설정을 반영하지 않고 사실만 알린다.
 */
export function isolationForced(runtime: TaskRuntime, agentCount: number): boolean {
  return runtime === "remote" || agentCount > 1;
}

export interface IsolationChipState {
  /** 이 프로젝트에 실제로 적용될 값. */
  on: boolean;
  /** 전역 기본이 아니라 이 프로젝트 전용 설정이 적용 중인가. */
  pinned: boolean;
}

/**
 * 격리 칩에 표시할 유효값과 그 출처. 오버라이드가 전역 기본을 이긴다.
 * 둘 다 모르는 동안(로드 전)에는 null — 격리 여부를 넘겨짚어 광고하지 않는다.
 */
export function isolationChipState(
  override: boolean | null,
  globalDefault: boolean | null,
): IsolationChipState | null {
  const on = override ?? globalDefault;
  if (on === null) return null;
  return { on, pinned: override !== null };
}

/** 격리 선택지 3종의 키. 순환 대신 목록으로 고른다. */
export type IsolationChoice = "default" | "pinned-on" | "pinned-off";

export interface IsolationOption {
  choice: IsolationChoice;
  label: string;
  /**
   * 무엇이 만들어지고 **무엇이 지워지는가.**
   *
   * 격리 실행의 워크트리와 브랜치는 승인하든 버리든 `cleanup_after_finalization()`이 함께
   * 지운다. "워크트리"라는 라벨만으로는 사용자가 그 사실에 닿을 길이 없고, 지워진 뒤에
   * 알게 된다. 이 캡션이 이 컴포넌트의 존재 이유다.
   */
  caption: string;
}

export function isolationOptions(globalDefault: boolean): IsolationOption[] {
  return [
    {
      choice: "default",
      label: globalDefault ? "워크트리 격리" : "직접 실행",
      caption: globalDefault
        ? "기본 설정을 따릅니다 — 새 브랜치와 격리 폴더를 만들고, 승인하거나 버리면 둘 다 삭제합니다."
        : "기본 설정을 따릅니다 — 선택한 브랜치의 메인 체크아웃에서 그대로 작업합니다.",
    },
    {
      choice: "pinned-on",
      label: "워크트리 격리 · 이 프로젝트만",
      caption: "이 레포에 고정합니다. 승인하거나 버리면 워크트리와 브랜치를 삭제합니다.",
    },
    {
      choice: "pinned-off",
      label: "직접 실행 · 이 프로젝트만",
      caption: "선택한 브랜치의 메인 체크아웃에서 작업합니다. 만드는 것이 없으니 지울 것도 없습니다.",
    },
  ];
}

export function choiceFromOverride(override: boolean | null): IsolationChoice {
  if (override === null) return "default";
  return override ? "pinned-on" : "pinned-off";
}

export function overrideFromChoice(choice: IsolationChoice): boolean | null {
  if (choice === "default") return null;
  return choice === "pinned-on";
}
