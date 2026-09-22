import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { openLocalFile, readLocalFile } from "./ipc";
import { registerTransport, unregisterTransport, type PraxisTransport } from "./transport";

const REMOTE_HOST = "local-file-ipc-remote";

beforeEach(() => {
  invoke.mockReset();
  unregisterTransport(REMOTE_HOST);
});

describe("openLocalFile", () => {
  it("로컬 task 좌표만 core 명령 DTO로 보낸다", async () => {
    await openLocalFile({ host: "local", id: 42 }, "docs/설계 문서/한글 파일.md");

    expect(invoke).toHaveBeenCalledWith("open_local_file", {
      id: 42,
      path: "docs/설계 문서/한글 파일.md",
    });
  });

  it("원격 host는 core IPC 전에 거부한다", async () => {
    registerTransport({ kind: "remote", hostId: REMOTE_HOST } as PraxisTransport);

    await expect(openLocalFile({ host: REMOTE_HOST, id: 7 }, "/home/me/report.md")).rejects.toThrow(
      "원격 작업에서는 열 수 없는 위치입니다",
    );

    expect(invoke).not.toHaveBeenCalled();
  });

  it("알려진 백엔드 오류의 기계 접두사를 숨긴 채 거부한다", async () => {
    invoke.mockRejectedValueOnce(new Error("local_file_denied: 경로가 허용되지 않습니다"));

    await expect(openLocalFile({ host: "local", id: 42 }, "../../outside.md")).rejects.toThrow(
      "경로가 허용되지 않습니다",
    );
  });

  it("알 수 없는 IPC 오류는 바꾸지 않는다", async () => {
    const error = new Error("Tauri unavailable");
    invoke.mockRejectedValueOnce(error);

    await expect(openLocalFile({ host: "local", id: 42 }, "docs/a.md")).rejects.toBe(error);
  });
});

describe("readLocalFile", () => {
  it("uses the local reader command for an external editor tab", async () => {
    invoke.mockResolvedValueOnce({ kind: "text", content: "note", mtime: 1 });

    await expect(readLocalFile({ host: "local", id: 42 }, "/Users/me/notes.md")).resolves.toEqual({
      kind: "text",
      content: "note",
      mtime: 1,
    });
    expect(invoke).toHaveBeenCalledWith("read_local_file", { id: 42, path: "/Users/me/notes.md" });
  });

  it("rejects remote paths before local IPC", async () => {
    registerTransport({ kind: "remote", hostId: REMOTE_HOST } as PraxisTransport);

    await expect(readLocalFile({ host: REMOTE_HOST, id: 7 }, "/Users/me/notes.md")).rejects.toThrow(
      "원격 작업에서는 열 수 없는 위치입니다",
    );
    expect(invoke).not.toHaveBeenCalled();
  });
});
