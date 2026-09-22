import { describe, expect, it } from "vitest";

import type { Task } from "./ipc";
import {
  assignProject,
  createGroup,
  canMoveGroup,
  dissolveGroup,
  EMPTY_PROJECT_GROUPS,
  moveGroup,
  orderSections,
  parseProjectGroups,
  renameGroup,
  serializeProjectGroups,
  setGroupColor,
  toggleGroup,
  unassignProject,
  type ProjectGroups,
} from "./project-groups";

const task = (repo: string, created_at: number): Task =>
  ({ id: created_at, host: "local", repo, created_at }) as Task;

const groups = (...ids: string[]): ProjectGroups => ({
  version: 1,
  groups: ids.map((id) => ({ id, name: id.toUpperCase(), collapsed: false })),
  assignment: {},
});

describe("parseProjectGroups", () => {
  it("저장이 없으면 빈 그룹이다", () => {
    expect(parseProjectGroups(null)).toEqual(EMPTY_PROJECT_GROUPS);
  });

  // 손상된 저장 하나가 사이드바를 통째로 막으면 프로젝트 목록도 못 본다.
  it.each([
    ["손상된 JSON", "{not json"],
    ["배열", "[]"],
    ["버전 불일치", '{"version":2,"groups":[],"assignment":{}}'],
    ["groups 형태 불일치", '{"version":1,"groups":[{"id":1}],"assignment":{}}'],
    ["assignment 형태 불일치", '{"version":1,"groups":[],"assignment":{"/a":7}}'],
  ])("%s은 던지지 않고 빈 그룹이 된다", (_label, raw) => {
    expect(parseProjectGroups(raw)).toEqual(EMPTY_PROJECT_GROUPS);
  });

  it("직렬화한 것을 그대로 되읽는다", () => {
    const saved: ProjectGroups = {
      version: 1,
      groups: [{ id: "g1", name: "사내용", collapsed: true }],
      assignment: { "/w/praxis": "g1" },
    };

    expect(parseProjectGroups(serializeProjectGroups(saved))).toEqual(saved);
  });

  it("알 수 없거나 잘못된 색상은 그룹과 배정을 보존한 채 버린다", () => {
    const raw = JSON.stringify({
      version: 1,
      groups: [
        { id: "root", name: "루트", collapsed: false, color: "blue" },
        { id: "child", name: "자식", collapsed: false, parentId: "root", color: "teal" },
        { id: "dangling", name: "고아", collapsed: false, parentId: "gone", color: 7 },
      ],
      assignment: { "/a": "child", "/b": "dangling" },
    });

    expect(parseProjectGroups(raw)).toEqual({
      version: 1,
      groups: [
        { id: "root", name: "루트", collapsed: false, color: "blue" },
        { id: "child", name: "자식", collapsed: false, parentId: "root" },
        { id: "dangling", name: "고아", collapsed: false, parentId: null },
      ],
      assignment: { "/a": "child", "/b": "dangling" },
    });
  });

  it("중복 id는 첫 레코드를 보존하고 순환은 배열 순서대로 하나씩 끊는다", () => {
    const raw = JSON.stringify({
      version: 1,
      groups: [
        { id: "a", name: "A", collapsed: false, parentId: "b" },
        { id: "b", name: "B", collapsed: false, parentId: "a" },
        { id: "a", name: "중복", collapsed: true },
      ],
      assignment: { "/a": "a", "/b": "b" },
    });

    expect(parseProjectGroups(raw)).toEqual({
      version: 1,
      groups: [
        { id: "a", name: "A", collapsed: false, parentId: null },
        { id: "b", name: "B", collapsed: false, parentId: "a" },
      ],
      assignment: { "/a": "a", "/b": "b" },
    });
  });
});

describe("그룹 조작", () => {
  it("id는 부르는 쪽이 주입한다 — 새 그룹은 맨 뒤에 붙는다", () => {
    const next = createGroup(groups("g1"), "개인용", "g2");

    expect(next.groups.map((group) => group.id)).toEqual(["g1", "g2"]);
    expect(next.groups[1]).toEqual({ id: "g2", name: "개인용", collapsed: false, parentId: null });
  });

  it("이름 바꾸기와 접기는 그 그룹만 건드린다", () => {
    const renamed = renameGroup(groups("g1", "g2"), "g2", "개인용");
    const toggled = toggleGroup(renamed, "g2");

    expect(toggled.groups.map((group) => [group.name, group.collapsed])).toEqual([
      ["G1", false],
      ["개인용", true],
    ]);
  });

  it("색상 변경과 복원은 대상 그룹만 건드리고 다른 조작에서도 유지한다", () => {
    const before = assignProject(groups("g1", "g2"), "/w/praxis", "g1");
    const colored = setGroupColor(before, "g1", "violet");
    const changed = toggleGroup(renameGroup(colored, "g1", "사내용"), "g1");
    const reset = setGroupColor(changed, "g1", undefined);

    expect(changed.groups).toEqual([
      { id: "g1", name: "사내용", collapsed: true, color: "violet" },
      { id: "g2", name: "G2", collapsed: false },
    ]);
    expect(reset.groups[0]).toEqual({ id: "g1", name: "사내용", collapsed: true });
    expect(reset.assignment).toEqual({ "/w/praxis": "g1" });
  });

  it("배정과 해제는 assignment만 바꾼다", () => {
    const assigned = assignProject(groups("g1"), "/w/praxis", "g1");

    expect(assigned.assignment).toEqual({ "/w/praxis": "g1" });
    expect(unassignProject(assigned, "/w/praxis").assignment).toEqual({});
  });

  it("이미 미소속인 프로젝트를 빼면 같은 상태를 돌려준다", () => {
    const before = groups("g1");

    expect(unassignProject(before, "/w/praxis")).toBe(before);
  });

  // 해제는 확인 없이 실행된다 — 프로젝트까지 사라지면 되돌릴 수 없다.
  it("그룹 해제는 직접 프로젝트와 자식을 부모로 승격한다", () => {
    const before = assignProject(assignProject(groups("g1", "g2"), "/a", "g1"), "/b", "g2");
    before.groups[1].parentId = "g1";

    const after = dissolveGroup(before, "g1");

    expect(after.groups.map((group) => group.id)).toEqual(["g2"]);
    expect(after.groups[0].parentId).toBeNull();
    expect(after.assignment).toEqual({ "/b": "g2" });
  });

  it("중간 그룹 해제는 직접 프로젝트도 상위 그룹으로 옮긴다", () => {
    const before = assignProject(groups("root", "middle", "child"), "/a", "middle");
    before.groups[1].parentId = "root";
    before.groups[2].parentId = "middle";

    const after = dissolveGroup(before, "middle");

    expect(after.assignment).toEqual({ "/a": "root" });
    expect(after.groups.find((group) => group.id === "child")?.parentId).toBe("root");
  });

  it("moveGroup은 목적지 형제 배열의 삽입 자리를 받는다", () => {
    const before = groups("g1", "g2", "g3");

    expect(moveGroup(before, "g3", null, 0).groups.map((g) => g.id)).toEqual(["g3", "g1", "g2"]);
    expect(moveGroup(before, "g1", null, 2).groups.map((g) => g.id)).toEqual(["g2", "g3", "g1"]);
  });

  it("자신·후손·없는 부모로는 이동하지 않는다", () => {
    const before = groups("g1", "g2", "g3");
    before.groups[1].parentId = "g1";

    expect(canMoveGroup(before, "g1", "g2")).toBe(false);
    expect(moveGroup(before, "g1", "g2")).toBe(before);
    expect(moveGroup(before, "g3", "gone")).toBe(before);
  });
});

describe("orderSections", () => {
  const tasks = [task("/w/praxis", 3), task("/w/infra", 9), task("/w/dotfiles", 1)];

  it("그룹이 없으면 구획은 미소속 하나뿐이다 — 지금과 같은 화면", () => {
    const sections = orderSections(["/w/praxis", "/w/infra"], tasks, EMPTY_PROJECT_GROUPS);

    expect(sections).toEqual([{ group: null, repos: ["/w/infra", "/w/praxis"], children: [] }]);
  });

  it("그룹 배열 순서대로 그리고 미소속이 마지막이다", () => {
    const state = assignProject(assignProject(groups("g1", "g2"), "/w/praxis", "g2"), "/w/infra", "g1");

    const sections = orderSections(["/w/praxis", "/w/infra", "/w/dotfiles"], tasks, state);

    expect(sections.map((section) => [section.group?.id ?? null, section.repos])).toEqual([
      ["g1", ["/w/infra"]],
      ["g2", ["/w/praxis"]],
      [null, ["/w/dotfiles"]],
    ]);
  });

  it("구획 안은 마지막 작업이 최근인 순서, 작업 없는 프로젝트는 맨 아래다", () => {
    const state = {
      ...groups("g1"),
      assignment: { "/w/praxis": "g1", "/w/infra": "g1", "/w/none": "g1" },
    };

    const [section] = orderSections(["/w/praxis", "/w/none", "/w/infra"], tasks, state);

    expect(section.repos).toEqual(["/w/infra", "/w/praxis", "/w/none"]);
  });

  // 렌더 쪽 방어(§5.4) — 제거된 프로젝트의 배정과 사라진 그룹 id가 목록을 망가뜨리지 않는다.
  it("없는 프로젝트의 배정은 세지 않고, 없는 그룹 id는 미소속으로 본다", () => {
    const state = {
      ...groups("g1"),
      assignment: { "/w/gone": "g1", "/w/praxis": "사라진그룹" },
    };

    const sections = orderSections(["/w/praxis"], tasks, state);

    expect(sections).toEqual([
      { group: state.groups[0], repos: [], children: [] },
      { group: null, repos: ["/w/praxis"], children: [] },
    ]);
  });

  it("자식 그룹을 직접 프로젝트보다 먼저 재귀로 그린다", () => {
    const state = assignProject(groups("g1", "g2"), "/w/praxis", "g1");
    state.groups[1].parentId = "g1";

    const [root] = orderSections(["/w/praxis"], tasks, state);

    expect(root.children.map((section) => section.group?.id)).toEqual(["g2"]);
    expect(root.repos).toEqual(["/w/praxis"]);
  });
});
