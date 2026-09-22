// @vitest-environment jsdom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  status: vi.fn(),
  configSet: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
  estimate: vi.fn(),
  sync: vi.fn(),
}));

vi.mock("../lib/ipc", () => ({
  knowledgeGmailStatus: mocks.status,
  knowledgeGmailConfigSet: mocks.configSet,
  knowledgeGmailConnect: mocks.connect,
  knowledgeGmailDisconnect: mocks.disconnect,
  knowledgeGmailEstimate: mocks.estimate,
  knowledgeGmailSync: mocks.sync,
}));

import { GmailSourceSection } from "./GmailSourceSection";

let container: HTMLDivElement;
let root: Root;

const statusOf = (over: Partial<Record<string, unknown>> = {}) => ({
  connected: false,
  client_id: "",
  client_secret_set: false,
  query: "-category:promotions newer_than:3y",
  stage: "미연결",
  indexed: 0,
  last_error: null,
  bundled_available: false,
  using_bundled: false,
  ...over,
});

const mount = async () => {
  await act(async () => {
    root.render(<GmailSourceSection />);
  });
};

const click = async (el: Element | null | undefined) => {
  expect(el, "클릭 대상이 없다").toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
};

const buttonBy = (label: string) =>
  Array.from(container.querySelectorAll("button")).find((b) =>
    b.textContent?.includes(label),
  );

/// 부분 일치가 다른 버튼을 삼킬 때 쓴다 — "연결"은 "개인 Google 계정으로 연결하기"에도 걸린다.
const exactButton = (label: string) =>
  Array.from(container.querySelectorAll("button")).find(
    (b) => b.textContent?.trim() === label,
  );

const clientIdInput = () =>
  container.querySelector('input[placeholder*="googleusercontent"]');

beforeEach(() => {
  vi.clearAllMocks();
  mocks.status.mockResolvedValue(statusOf());
  mocks.configSet.mockResolvedValue(undefined);
  mocks.disconnect.mockResolvedValue(0);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});

describe("GmailSourceSection", () => {
  it("연결 전에는 파괴적·동기화 동작을 아예 노출하지 않는다", async () => {
    await mount();
    expect(buttonBy("연결 해제")).toBeUndefined();
    expect(buttonBy("동기화")).toBeUndefined();
    expect(buttonBy("연결")).toBeTruthy();
  });

  /// client_id 없이 연결을 시도하면 백엔드가 거부한다. 눌리지 않게 막아 왕복을 아낀다.
  it("client_id가 없으면 연결 버튼이 비활성이다", async () => {
    await mount();
    expect((buttonBy("연결") as HTMLButtonElement).disabled).toBe(true);
  });

  // ── 번들 client (ADR 0147) ──

  /// 배포받은 사용자가 겪는 기본 경로. 여기서 입력란이 보이면 "뭘 넣어야 하지"로 되돌아간다.
  it("번들이 있으면 입력란을 감추고 바로 연결할 수 있다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ bundled_available: true, using_bundled: true }),
    );
    await mount();

    expect(clientIdInput()).toBeNull();
    expect(container.textContent).toContain("따로 설정할 것이 없습니다");
    // client_id가 비어 있어도 번들이 있으면 눌려야 한다.
    expect((exactButton("연결") as HTMLButtonElement).disabled).toBe(false);
  });

  /// 번들이 받아주지 않는 계정이 있다. 번들이 직접 입력 경로를 막으면 안 된다.
  it("직접 입력 경로를 펼치면 입력란이 돌아온다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ bundled_available: true, using_bundled: true }),
    );
    await mount();
    expect(clientIdInput()).toBeNull();

    await click(buttonBy("다른 클라이언트 직접 입력하기"));

    expect(clientIdInput()).toBeTruthy();
    expect(buttonBy("Google Cloud 설정 절차 보기")).toBeTruthy();
  });

  /// 이미 자기 값을 넣어 둔 사용자에게 접힌 화면을 보여주면 설정이 사라진 것처럼 보인다.
  it("사용자 client를 쓰는 중이면 접지 않고 되돌릴 길을 준다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ bundled_available: true, using_bundled: false, client_id: "mine" }),
    );
    await mount();

    expect(clientIdInput()).toBeTruthy();
    expect(buttonBy("기본 클라이언트로 되돌리기")).toBeTruthy();
  });

  /// client_id를 비워 저장하면 백엔드 해석이 번들로 돌아간다.
  it("되돌리기는 client_id를 비워 저장한다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ bundled_available: true, using_bundled: false, client_id: "mine" }),
    );
    await mount();

    await click(buttonBy("기본 클라이언트로 되돌리기"));

    expect(mocks.configSet).toHaveBeenCalledWith("", expect.any(String));
  });

  /// 비활성 버튼만 있고 이유가 없으면 사용자는 화면이 고장 났다고 읽는다.
  it("번들도 없고 client_id도 없으면 무엇을 해야 하는지 말해준다", async () => {
    await mount();
    expect(container.textContent).toContain("client_id");
    expect(container.textContent).toContain("입력하고 저장하세요");
  });

  it("연결되면 지워질 건수를 확인 문구에 담아 묻는다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ connected: true, client_id: "abc", indexed: 1234, stage: "최신" }),
    );
    const confirmed = vi.spyOn(window, "confirm").mockReturnValue(false);
    await mount();

    await click(buttonBy("연결 해제"));

    expect(confirmed).toHaveBeenCalledWith(expect.stringContaining("1234"));
    // 거부했으면 아무것도 지우지 않는다.
    expect(mocks.disconnect).not.toHaveBeenCalled();
    confirmed.mockRestore();
  });

  it("확인을 승인해야 실제로 해제한다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ connected: true, client_id: "abc", indexed: 3 }),
    );
    const confirmed = vi.spyOn(window, "confirm").mockReturnValue(true);
    await mount();

    await click(buttonBy("연결 해제"));

    expect(mocks.disconnect).toHaveBeenCalledTimes(1);
    confirmed.mockRestore();
  });

  /// secret은 화면이 되읽을 수 없다. 저장할 때마다 다시 입력하게 하면
  /// 필터만 고치려던 사용자가 연결을 잃는다.
  it("secret을 비운 채 저장하면 기존 값을 건드리지 않는다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ connected: true, client_id: "abc", client_secret_set: true }),
    );
    await mount();

    await click(buttonBy("저장"));

    expect(mocks.configSet).toHaveBeenCalledWith("abc", expect.any(String), undefined);
  });

  it("백엔드가 남긴 마지막 오류를 화면에 띄운다", async () => {
    mocks.status.mockResolvedValue(
      statusOf({ connected: true, client_id: "abc", last_error: "할당량 초과" }),
    );
    await mount();
    expect(container.textContent).toContain("할당량 초과");
  });
});
