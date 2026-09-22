import { describe, expect, it } from "vitest";

import {
  ALL_REPOS,
  hasNotice,
  issueRows,
  issueUrl,
  mergedStatus,
  pickRepo,
  repoLabels,
  reposWithIssues,
  toRepoIssues,
  withoutIssue,
  type RepoIssues,
} from "./github-repos";
import type { GhIssue, GhRepo } from "../../lib/ipc";

const repo = (path: string, ownerRepo: string): GhRepo => ({ path, owner_repo: ownerRepo });

const anIssue: GhIssue = {
  number: 1,
  title: "이슈",
  labels: [],
  updated_at: "2026-08-03T00:00:00Z",
};

describe("repoLabels", () => {
  it("이름이 겹치지 않으면 repo만 쓴다", () => {
    const labels = repoLabels([repo("/a", "acme/app"), repo("/b", "acme/site")]);
    expect(labels.get("/a")).toBe("app");
    expect(labels.get("/b")).toBe("site");
  });

  it("겹치는 이름만 owner까지 붙인다 — fork를 구분하되 나머지는 짧게 둔다", () => {
    const labels = repoLabels([
      repo("/a", "acme/app"),
      repo("/b", "fork/app"),
      repo("/c", "acme/site"),
    ]);
    expect(labels.get("/a")).toBe("acme/app");
    expect(labels.get("/b")).toBe("fork/app");
    expect(labels.get("/c")).toBe("site");
  });
});

describe("pickRepo", () => {
  it("선호 경로가 후보에 있으면 그것을 고른다", () => {
    expect(pickRepo([repo("/a", "acme/app"), repo("/b", "acme/site")], "/b")).toBe("/b");
  });

  it("선호가 GitHub 레포가 아니면 첫 후보로 떨어진다", () => {
    expect(pickRepo([repo("/a", "acme/app")], "/not-github")).toBe("/a");
    expect(pickRepo([repo("/a", "acme/app")], undefined)).toBe("/a");
  });

  it("후보가 없으면 undefined — 섹션 자체가 그려지지 않는다", () => {
    expect(pickRepo([], "/a")).toBeUndefined();
  });
});

describe("reposWithIssues", () => {
  const REPOS = [repo("/a", "acme/app"), repo("/b", "acme/site")];
  const results = (entries: Record<string, RepoIssues>) => new Map(Object.entries(entries));
  const withIssues: RepoIssues = { status: "ready", issues: [anIssue], ownerRepo: "acme/app" };
  const empty: RepoIssues = { status: "ready", issues: [], ownerRepo: "acme/site" };

  it("열린 이슈가 0건으로 확인된 레포를 뺀다", () => {
    const kept = reposWithIssues(REPOS, results({ "/a": withIssues, "/b": empty }));
    expect(kept.map((r) => r.path)).toEqual(["/a"]);
  });

  it("아직 응답이 없는 레포는 남긴다 — 판정 전은 '이슈 없음'이 아니다", () => {
    const kept = reposWithIssues(REPOS, results({ "/a": withIssues }));
    expect(kept.map((r) => r.path)).toEqual(["/a", "/b"]);
  });

  it("조회에 실패한 레포는 뺀다 — 눌러도 이슈가 나오지 않는다", () => {
    const failed: RepoIssues = { status: "error", issues: [], ownerRepo: null };
    const kept = reposWithIssues(REPOS, results({ "/a": withIssues, "/b": failed }));
    expect(kept.map((r) => r.path)).toEqual(["/a"]);
  });

  it("GitHub 원격이 없는 레포도 뺀다", () => {
    const notGithub: RepoIssues = { status: "not_github_repo", issues: [], ownerRepo: null };
    const kept = reposWithIssues(REPOS, results({ "/a": withIssues, "/b": notGithub }));
    expect(kept.map((r) => r.path)).toEqual(["/a"]);
  });

  it("gh 자체가 없으면 아무것도 걸러내지 않는다 — 판정이 성립하지 않는다", () => {
    const down: RepoIssues = { status: "unavailable", issues: [], ownerRepo: null };
    const kept = reposWithIssues(REPOS, results({ "/a": down, "/b": empty }));
    expect(kept).toEqual(REPOS);
  });

  it("모두 0건이면 아무 레포도 남지 않는다", () => {
    const kept = reposWithIssues(REPOS, results({ "/a": empty, "/b": empty }));
    expect(kept).toEqual([]);
  });
});

describe("hasNotice", () => {
  const results = (entries: Record<string, RepoIssues>) => new Map(Object.entries(entries));

  it("전부 정상 조회면 말할 것이 없다 — 섹션을 접어도 된다", () => {
    expect(
      hasNotice(
        results({
          "/a": { status: "ready", issues: [anIssue], ownerRepo: "acme/app" },
          "/b": { status: "ready", issues: [], ownerRepo: "acme/site" },
        }),
      ),
    ).toBe(false);
  });

  it("gh 부재·조회 실패는 말해야 한다 — 사용자가 고칠 수 있는 사정이다", () => {
    const down: RepoIssues = { status: "unavailable", issues: [], ownerRepo: null };
    const failed: RepoIssues = { status: "error", issues: [], ownerRepo: null };
    expect(hasNotice(results({ "/a": down }))).toBe(true);
    expect(hasNotice(results({ "/a": failed }))).toBe(true);
  });
});

describe("toRepoIssues", () => {
  it("ready는 목록과 이름을 그대로 옮긴다", () => {
    expect(toRepoIssues({ status: "ready", owner_repo: "acme/app", issues: [anIssue] })).toEqual({
      status: "ready",
      issues: [anIssue],
      ownerRepo: "acme/app",
    });
  });

  it("ready가 아니면 보여줄 목록도 이름도 없다", () => {
    expect(toRepoIssues({ status: "unavailable" })).toEqual({
      status: "unavailable",
      issues: [],
      ownerRepo: null,
    });
  });
});

describe("issueUrl", () => {
  it("owner/repo와 번호로 이슈 주소를 만든다", () => {
    expect(issueUrl("acme/app", 42)).toBe("https://github.com/acme/app/issues/42");
  });
});

describe("pickRepo — 전체 보기", () => {
  it("'전체'는 경로가 아니어도 그대로 유지된다 — 아니면 첫 레포로 되돌아간다", () => {
    expect(pickRepo([repo("/a", "acme/app"), repo("/b", "acme/site")], ALL_REPOS)).toBe(ALL_REPOS);
  });

  it("후보가 없으면 '전체'라도 undefined — 그릴 섹션이 없다", () => {
    expect(pickRepo([], ALL_REPOS)).toBeUndefined();
  });
});

describe("issueRows", () => {
  const REPOS = [repo("/a", "acme/app"), repo("/b", "acme/site")];
  const at = (number: number, updatedAt: string): GhIssue => ({ ...anIssue, number, updated_at: updatedAt });
  const results = new Map<string, RepoIssues>([
    [
      "/a",
      {
        status: "ready",
        issues: [at(1, "2026-08-01T00:00:00Z"), at(3, "2026-08-10T00:00:00Z")],
        ownerRepo: "acme/app",
      },
    ],
    ["/b", { status: "ready", issues: [at(2, "2026-08-05T00:00:00Z")], ownerRepo: "acme/site" }],
  ]);

  it("레포 하나를 고르면 그 레포만, gh가 준 순서 그대로", () => {
    expect(issueRows(REPOS, results, "/a").map((row) => row.issue.number)).toEqual([1, 3]);
  });

  it("전체는 후보를 합쳐 최근 갱신순으로 — 레포별로 이어 붙이면 오늘 것이 아래로 간다", () => {
    expect(issueRows(REPOS, results, ALL_REPOS).map((row) => row.issue.number)).toEqual([3, 2, 1]);
  });

  it("줄마다 제 레포를 들고 있다 — 전체 보기의 태스크 생성·삭제가 엉뚱한 레포로 가면 안 된다", () => {
    const rows = issueRows(REPOS, results, ALL_REPOS);
    expect(rows.map((row) => row.repoPath)).toEqual(["/a", "/b", "/a"]);
    expect(rows[1].ownerRepo).toBe("acme/site");
  });

  it("ready가 아닌 레포는 전체에서도 줄을 내지 않는다", () => {
    const partial = new Map<string, RepoIssues>([
      ["/a", { status: "error", issues: [], ownerRepo: null }],
      ["/b", { status: "ready", issues: [at(2, "2026-08-05T00:00:00Z")], ownerRepo: "acme/site" }],
    ]);
    expect(issueRows(REPOS, partial, ALL_REPOS).map((row) => row.issue.number)).toEqual([2]);
  });
});

describe("mergedStatus", () => {
  const REPOS = [repo("/a", "acme/app"), repo("/b", "acme/site")];
  const ready: RepoIssues = { status: "ready", issues: [anIssue], ownerRepo: "acme/app" };

  it("하나라도 받았으면 ready — 한 레포의 실패가 나머지를 감추면 안 된다", () => {
    const results = new Map<string, RepoIssues>([
      ["/a", { status: "error", issues: [], ownerRepo: null }],
      ["/b", ready],
    ]);
    expect(mergedStatus(REPOS, results)).toBe("ready");
  });

  it("전부 실패하면 고칠 수 있는 사정(gh 부재)을 조회 실패보다 앞세운다", () => {
    const results = new Map<string, RepoIssues>([
      ["/a", { status: "error", issues: [], ownerRepo: null }],
      ["/b", { status: "unavailable", issues: [], ownerRepo: null }],
    ]);
    expect(mergedStatus(REPOS, results)).toBe("unavailable");
  });

  it("응답이 하나도 없으면 loading — 빈 목록으로 단정하지 않는다", () => {
    expect(mergedStatus(REPOS, new Map())).toBe("loading");
  });
});

describe("withoutIssue", () => {
  const results = new Map<string, RepoIssues>([
    [
      "/a",
      { status: "ready", issues: [anIssue, { ...anIssue, number: 2 }], ownerRepo: "acme/app" },
    ],
  ]);

  it("지운 한 건만 빠지고 나머지는 남는다", () => {
    expect(withoutIssue(results, "/a", 1).get("/a")?.issues.map((issue) => issue.number)).toEqual([2]);
  });

  it("원본을 바꾸지 않는다 — 스냅샷 교체가 렌더를 태워야 한다", () => {
    const next = withoutIssue(results, "/a", 1);
    expect(next).not.toBe(results);
    expect(results.get("/a")?.issues).toHaveLength(2);
  });

  it("모르는 레포면 그대로 — 없는 캐시를 새로 만들지 않는다", () => {
    expect(withoutIssue(results, "/zzz", 1)).toBe(results);
  });
});
