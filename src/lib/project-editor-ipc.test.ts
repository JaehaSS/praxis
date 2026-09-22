import { describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const ipc = await import("./project-editor-ipc");

describe("project editor IPC", () => {
  it("uses the frozen root-bound command names and camelCase write payload", () => {
    ipc.projectEditorOpen("/repo");
    ipc.projectEditorWrite("src/a.ts", "next", "before");
    ipc.projectEditorOpenPath("src/a.ts");
    ipc.projectShellResize(4, 100, 30);

    expect(invoke).toHaveBeenNthCalledWith(1, "project_editor_open", { root: "/repo" });
    expect(invoke).toHaveBeenNthCalledWith(2, "project_editor_write", { path: "src/a.ts", content: "next", expectedContent: "before" });
    expect(invoke).toHaveBeenNthCalledWith(3, "project_editor_open_path", { path: "src/a.ts" });
    expect(invoke).toHaveBeenNthCalledWith(4, "project_editor_shell_resize", { session: 4, cols: 100, rows: 30 });
  });
});
