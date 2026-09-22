// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HostScopeProvider } from "../lib/host-scope";

const mocks = vi.hoisted(() => ({ list: vi.fn(), read: vi.fn() }));
vi.mock("../lib/harness-experience", async (original) => ({
  ...(await original<typeof import("../lib/harness-experience")>()),
  loadExperiences: mocks.list,
  readExperience: mocks.read,
}));
import { HarnessExperiencePanel } from "./HarnessExperiencePanel";

let host: HTMLDivElement;
let root: Root;
(
  globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.list.mockReset();
  mocks.read.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("HarnessExperiencePanel", () => {
  it("clears content and makes no service read when browsing host becomes remote", async () => {
    mocks.list.mockResolvedValue({
      state: "unsupported",
      reason: "remote-host",
    });
    await act(async () =>
      root.render(
        <HostScopeProvider value="remote">
          <HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />
        </HostScopeProvider>,
      ),
    );
    expect(mocks.list).toHaveBeenCalledWith(
      "remote",
      "/repo",
      "workflow-harness",
    );
    expect(mocks.read).not.toHaveBeenCalled();
    expect(host.textContent).toContain("로컬 프로젝트에서만 지원");
  });

  it("keeps the selected source and page while reading a document", async () => {
    const documents = Array.from({ length: 21 }, (_, index) => ({
      key: `memory/doc-${index + 1}`,
      displayPath: `docs/memory/doc-${index + 1}.md`,
      relation: "project-reference" as const,
    }));
    mocks.list.mockResolvedValue({
      state: "ready",
      observedAt: "now",
      sources: [
        {
          source: {
            host: "local",
            harness: "workflow-harness",
            vendor: "codex",
            scope: "global",
          },
          state: "ready",
          documents,
          limited: false,
          inspectedEntries: 21,
        },
      ],
      project: {
        project: { host: "local", projectKey: "/repo" },
        state: "empty",
      },
    });
    mocks.read.mockResolvedValue({
      state: "ready",
      document: {
        owner: {
          kind: "skill",
          source: {
            host: "local",
            harness: "workflow-harness",
            vendor: "codex",
            scope: "global",
          },
        },
        documentKey: "memory/doc-21",
        path: "/very/long/docs/memory/doc-21.md",
        contentHash: "hash",
        observedAt: "now",
        generated: false,
        sourceResolution: "not-applicable",
        text: "excerpt",
      },
    });

    await act(async () => {
      root.render(<HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />);
      await Promise.resolve();
    });
    await act(async () => button("codex · global")?.click());
    await act(async () => button("다음")?.click());
    await act(async () => button("docs/memory/doc-21.md")?.click());

    expect(host.textContent).toContain("2 / 2");
    expect(
      Array.from(host.querySelectorAll("button")).find(
        (item) => item.textContent === "codex · global",
      )?.getAttribute("aria-pressed"),
    ).toBe("true");
  });

  it("blocks changed excerpts and distinguishes a reread failure", async () => {
    const writeText = vi.fn();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    mocks.list.mockResolvedValue(readyList());
    mocks.read
      .mockResolvedValueOnce(readyRead())
      .mockResolvedValueOnce({ state: "error", error: "io-error" });

    await openDocument();
    await act(async () => setExcerpt("excerpt"));
    await act(async () => button("초안 복사")?.click());

    expect(writeText).not.toHaveBeenCalled();
    expect(host.textContent).toContain("원문을 다시 읽지 못했습니다: 파일 읽기 오류가 발생했습니다.");
  });

  it("blocks copying when the reread hash changed", async () => {
    const writeText = vi.fn();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    mocks.list.mockResolvedValue(readyList());
    mocks.read
      .mockResolvedValueOnce(readyRead())
      .mockResolvedValueOnce({
        ...readyRead(),
        document: { ...readyRead().document, contentHash: "new-hash" },
      });

    await openDocument();
    await act(async () => setExcerpt("excerpt"));
    await act(async () => button("초안 복사")?.click());

    expect(writeText).not.toHaveBeenCalled();
    expect(host.textContent).toContain("원문이 변경되었습니다. 다시 확인한 뒤 복사하세요.");
  });

  it("copies the path only when clipboard support is available", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: undefined,
    });
    mocks.list.mockResolvedValue(readyList());
    mocks.read.mockResolvedValue(readyRead());

    await openDocument();

    expect(button("경로 복사")?.disabled).toBe(true);
    expect(button("초안 복사")?.disabled).toBe(true);
  });

  it("reports a clipboard write failure and lists generated-source guidance", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
    });
    mocks.list.mockResolvedValue(readyList());
    mocks.read.mockResolvedValue(readyRead());

    await openDocument();
    await act(async () => button("경로 복사")?.click());

    expect(host.textContent).toContain("경로를 복사하지 못했습니다.");
    expect(host.textContent).toContain("docs/memory, docs/archive, docs/lessons-seed.md");
  });

  it("copies a verified draft", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    mocks.list.mockResolvedValue(readyList());
    mocks.read.mockResolvedValue(readyRead());

    await openDocument();
    await act(async () => setExcerpt("excerpt"));
    await act(async () => button("초안 복사")?.click());

    expect(writeText).toHaveBeenCalledWith(
      expect.stringContaining("검토할 구절: excerpt"),
    );
  });

  it("copies a path and discards a pending draft copy after source change", async () => {
    const draftRead = deferred<ReturnType<typeof readyRead>>();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    mocks.list.mockResolvedValue(readyList());
    mocks.read.mockResolvedValueOnce(readyRead()).mockReturnValueOnce(draftRead.promise);

    await openDocument();
    await act(async () => button("경로 복사")?.click());
    expect(writeText).toHaveBeenCalledWith("/repo/docs/memory/example.md");

    await act(async () => setExcerpt("excerpt"));
    await act(async () => {
      button("초안 복사")?.click();
      button("초안 복사")?.click();
    });
    expect(mocks.read).toHaveBeenCalledTimes(2);
    await act(async () => button("claude · project")?.click());
    await act(async () => draftRead.resolve(readyRead()));

    expect(writeText).toHaveBeenCalledTimes(1);
    expect(host.textContent).not.toContain("복사됨");
  });

  it("discards a late local list after switching to remote", async () => {
    const localList = deferred<ReturnType<typeof readyList>>();
    mocks.list.mockImplementation((hostName: string) =>
      hostName === "local"
        ? localList.promise
        : Promise.resolve({ state: "unsupported", reason: "remote-host" }),
    );

    await act(async () => {
      root.render(
        <HostScopeProvider value="local">
          <HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />
        </HostScopeProvider>,
      );
    });
    await act(async () => {
      root.render(
        <HostScopeProvider value="remote">
          <HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />
        </HostScopeProvider>,
      );
    });
    await act(async () => localList.resolve(readyList()));

    expect(host.textContent).toContain("로컬 프로젝트에서만 지원");
    expect(host.textContent).not.toContain("docs/memory/example.md");
  });

  it("does not publish a completed read after the host switches", async () => {
    let resolveRead: (value: ReturnType<typeof readyRead>) => void;
    mocks.list.mockImplementation((hostName: string) =>
      Promise.resolve(
        hostName === "local"
          ? readyList()
          : { state: "unsupported", reason: "remote-host" },
      ),
    );
    mocks.read.mockReturnValue(
      new Promise((resolve) => {
        resolveRead = resolve;
      }),
    );

    await act(async () => {
      root.render(
        <HostScopeProvider value="local">
          <HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />
        </HostScopeProvider>,
      );
      await Promise.resolve();
    });
    await act(async () => button("docs/memory/example.md")?.click());
    await act(async () => {
      root.render(
        <HostScopeProvider value="remote">
          <HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />
        </HostScopeProvider>,
      );
    });
    await act(async () => resolveRead!(readyRead()));

    expect(host.textContent).not.toContain("sha256:hash");
  });

  it("discards a late read after switching sources or unmounting", async () => {
    const pendingRead = deferred<ReturnType<typeof readyRead>>();
    mocks.list.mockResolvedValue(readyList());
    mocks.read.mockReturnValue(pendingRead.promise);

    await openDocument();
    await act(async () => button("claude · project")?.click());
    await act(async () => pendingRead.resolve(readyRead()));

    expect(host.textContent).not.toContain("sha256:hash");

    await act(async () => root.unmount());
  });
});

function readyList() {
  return {
    state: "ready" as const,
    observedAt: "now",
    sources: [
      {
        source: {
          host: "local" as const,
          harness: "workflow-harness" as const,
          vendor: "claude" as const,
          scope: "project" as const,
        },
        state: "ready" as const,
        documents: [],
        limited: false,
        inspectedEntries: 0,
      },
    ],
    project: {
      project: { host: "local" as const, projectKey: "/repo" },
      state: "ready" as const,
      documents: [
        {
          key: "memory/example",
          displayPath: "docs/memory/example.md",
          relation: "project-reference" as const,
        },
      ],
      limited: false,
      inspectedEntries: 1,
    },
  };
}

function readyRead() {
  return {
    state: "ready" as const,
    document: {
      owner: {
        kind: "project" as const,
        project: { host: "local" as const, projectKey: "/repo" },
      },
      documentKey: "memory/example",
      path: "/repo/docs/memory/example.md",
      contentHash: "hash",
      observedAt: "now",
      generated: true,
      sourceResolution: "known-set" as const,
      text: "excerpt",
    },
  };
}

function button(label: string): HTMLButtonElement | undefined {
  return Array.from(host.querySelectorAll("button")).find(
    (item) => item.textContent === label,
  );
}

async function openDocument() {
  await act(async () => {
    root.render(<HarnessExperiencePanel repo="/repo" onOpenMemory={() => {}} />);
    await Promise.resolve();
  });
  await act(async () => button("프로젝트 참고")?.click());
  await act(async () => button("docs/memory/example.md")?.click());
}

function setExcerpt(value: string) {
  const textarea = host.querySelector("textarea");
  if (!textarea) throw new Error("excerpt textarea missing");
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )?.set;
  setter?.call(textarea, value);
  textarea.dispatchEvent(new Event("input", { bubbles: true }));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}
