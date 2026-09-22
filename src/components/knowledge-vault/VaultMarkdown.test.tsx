// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import { VaultMarkdown } from "./VaultMarkdown";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let node: HTMLDivElement;
let root: Root;

beforeEach(() => {
  node = document.createElement("div");
  document.body.appendChild(node);
  root = createRoot(node);
});

afterEach(() => {
  act(() => root.unmount());
  node.remove();
});

it("renders markdown without injecting raw HTML", async () => {
  await act(async () => { root.render(<VaultMarkdown body={'# Title\n\n<script>window.pwned = true</script>\n\n**safe**'} />); });

  expect(node.querySelector("script")).toBeNull();
  expect(node.textContent).toContain("safe");
});

it("does not load remote images or turn links into navigations", async () => {
  await act(async () => { root.render(<VaultMarkdown body={'[outside](https://example.test)\n\n![tracker](https://example.test/pixel.png)'} />); });

  expect(node.querySelector("a, img")).toBeNull();
  expect(node.textContent).toContain("[이미지: tracker]");
});
