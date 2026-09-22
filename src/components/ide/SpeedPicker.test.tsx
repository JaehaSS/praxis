// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { SpeedPicker } from "./SpeedPicker";

const api = vi.hoisted(() => ({ models: vi.fn() }));
vi.mock("../../lib/ipc", () => ({ codexSpeedModels: api.models }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  container = document.createElement("div"); document.body.append(container);
  root = createRoot(container);
  api.models.mockReset().mockResolvedValue(["gpt-6-astra"]);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); });

it("selects Fast for a supported explicit model, then returns to Standard", async () => {
  const change = vi.fn();
  await act(async () => root.render(<SpeedPicker model="gpt-6-astra" value="default" onChange={change} />));
  const select = container.querySelector("select")!;
  expect(select.title).toContain("다음 메시지");
  expect(select.querySelector<HTMLOptionElement>('[value="fast"]')!.disabled).toBe(false);
  await act(async () => { select.value = "fast"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(change).toHaveBeenLastCalledWith("fast");
  await act(async () => root.render(<SpeedPicker model="gpt-6-astra" value="fast" onChange={change} />));
  await act(async () => { select.value = "default"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(change).toHaveBeenLastCalledWith("default");
});

it.each(["", "unknown-model"])("cannot enable Fast without capability for %s", async (model) => {
  const change = vi.fn();
  await act(async () => root.render(<SpeedPicker model={model} value={null} onChange={change} />));
  const select = container.querySelector("select")!;
  expect(select.value).toBe(""); // Do not mislabel an inherited CLI setting as Standard.
  expect(select.querySelector<HTMLOptionElement>('[value="fast"]')!.disabled).toBe(true);
  await act(async () => { select.value = "fast"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(change).not.toHaveBeenCalled();
});

it("allows Standard when reading capabilities fails", async () => {
  api.models.mockRejectedValue(new Error("unavailable"));
  const change = vi.fn();
  await act(async () => root.render(<SpeedPicker model="gpt-6-astra" value={null} onChange={change} />));
  const select = container.querySelector("select")!;
  expect(select.title).toContain("읽지 못했습니다");
  await act(async () => { select.value = "default"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(change).toHaveBeenCalledWith("default");
});
