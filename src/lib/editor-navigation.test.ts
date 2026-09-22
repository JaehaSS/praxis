import { describe, expect, it } from "vitest";

import { currentNavigation, emptyNavigation, moveNavigation, pushNavigation } from "./editor-navigation";

const location = (path: string, line: number) => ({ path, groupId: "g1", line, column: 1, scrollTop: 0, scrollLeft: 0 });

describe("editor navigation", () => {
  it("returns through prior locations and discards forward history after a new navigation", () => {
    let state = emptyNavigation();
    state = pushNavigation(state, location("a.ts", 1));
    state = pushNavigation(state, location("b.ts", 2));
    state = pushNavigation(state, location("c.ts", 3));
    state = moveNavigation(state, -1);
    state = moveNavigation(state, -1);
    expect(currentNavigation(state)).toEqual(location("a.ts", 1));

    state = pushNavigation(state, location("d.ts", 4));
    expect(state.entries.map((entry) => entry.path)).toEqual(["a.ts", "d.ts"]);
  });

  it("deduplicates adjacent positions and bounds history to 100 entries", () => {
    let state = pushNavigation(emptyNavigation(), location("a.ts", 1));
    state = pushNavigation(state, location("a.ts", 1));
    for (let line = 2; line <= 101; line += 1) state = pushNavigation(state, location("a.ts", line));

    expect(state.entries).toHaveLength(100);
    expect(state.entries[0]).toEqual(location("a.ts", 2));
  });

  it("keeps the newest scroll position for an adjacent duplicate and leaves empty history empty", () => {
    const first = { ...location("a.ts", 1), scrollTop: 12 };
    const latest = { ...first, scrollTop: 48 };
    const state = pushNavigation(pushNavigation(emptyNavigation(), first), latest);

    expect(state.entries).toEqual([latest]);
    expect(moveNavigation(emptyNavigation(), -1)).toEqual(emptyNavigation());
  });
});
