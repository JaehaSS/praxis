import type { GhIssue, GhRepo, GithubIssuesResult } from "../../lib/ipc";

/** `owner/repo` → `repo`. 슬래시가 없으면 원문 그대로. */
const shortName = (ownerRepo: string): string => ownerRepo.split("/").pop() || ownerRepo;

/**
 * 레포 버튼에 쓸 라벨(경로 → 라벨). 기본은 `repo`만 — 버튼 줄이 헤더 한 줄에 들어가야 한다.
 *
 * 다만 fork나 동명 레포가 섞이면 `repo`만으로는 같은 버튼이 두 개 뜬 것처럼 보인다.
 * 그 이름에 한해서만 `owner/repo`로 늘려 구분한다 — 전부 늘리면 흔한 경우가 손해를 본다.
 */
export function repoLabels(repos: GhRepo[]): Map<string, string> {
  const counts = new Map<string, number>();
  for (const repo of repos) {
    const short = shortName(repo.owner_repo);
    counts.set(short, (counts.get(short) ?? 0) + 1);
  }
  return new Map(
    repos.map((repo) => {
      const short = shortName(repo.owner_repo);
      return [repo.path, (counts.get(short) ?? 0) > 1 ? repo.owner_repo : short];
    }),
  );
}

/** 레포 하나의 이슈 조회 결과. 섹션이 레포별로 들고 있어 레포를 오가도 다시 부르지 않는다. */
export interface RepoIssues {
  status: GithubIssuesResult["status"] | "error";
  issues: GhIssue[];
  ownerRepo: string | null;
}

/** 백엔드 응답 → 캐시 항목. `ready`가 아니면 보여줄 목록도 이름도 없다. */
export function toRepoIssues(result: GithubIssuesResult): RepoIssues {
  return result.status === "ready"
    ? { status: "ready", issues: result.issues, ownerRepo: result.owner_repo }
    : { status: result.status, issues: [], ownerRepo: null };
}

/**
 * 버튼에 남길 레포 — 열린 이슈가 **있는 것으로 확인된** 레포만.
 *
 * 남는 근거는 `ready && 1건 이상`, 그리고 아직 응답이 오지 않은 것뿐이다. 조회에 실패한
 * 레포(`error`·`not_github_repo`)도 뺀다 — 눌러 봐야 나오는 것은 빈 목록이나 에러 카드라,
 * 화면에서는 이슈가 없는 레포와 구분되지 않는다. 판정 전을 남기는 것은 다른 이야기다.
 * 응답이 오기 전에 지우면 후보 전부를 그렸다가 하나씩 사라지는 장면이 된다.
 *
 * gh 자체가 죽어 있으면(`unavailable`) 어느 레포도 판정되지 않으므로 후보를 그대로 두고
 * 안내 카드가 이유를 말하게 한다 — 고칠 수 있는 사정을 조용히 숨기지 않는다.
 */
export function reposWithIssues(repos: GhRepo[], results: Map<string, RepoIssues>): GhRepo[] {
  if ([...results.values()].some((result) => result.status === "unavailable")) return repos;
  return repos.filter((repo) => {
    const result = results.get(repo.path);
    if (!result) return true;
    return result.status === "ready" && result.issues.length > 0;
  });
}

/**
 * 사용자에게 말해야 할 사정이 섞여 있는지 — gh 부재·조회 실패처럼 **사용자가 고칠 수 있는** 것.
 *
 * 보여줄 레포가 하나도 없을 때 섹션을 접을지 남길지를 가른다. 이유 없이 빈 섹션이 홈 위쪽
 * 자리를 지키는 것도, 고칠 수 있는 실패가 말없이 사라지는 것도 피한다.
 */
export function hasNotice(results: Map<string, RepoIssues>): boolean {
  return [...results.values()].some((result) => result.status !== "ready");
}

/**
 * 실제로 볼 레포. 선호 경로가 후보에 있으면 그것, 없으면 첫 후보로 떨어진다.
 *
 * 폴백이 필요한 이유: 홈은 "가장 최근 작업의 레포"를 선호로 넘기는데 그게 GitHub 레포가
 * 아닐 수 있다. 그때 빈손으로 두면 GitHub 레포를 여럿 쓰는데도 섹션이 비어 보인다.
 */
export function pickRepo(repos: GhRepo[], preferred: string | undefined): string | undefined {
  if (repos.length === 0) return undefined;
  // '전체'는 어느 경로와도 맞지 않으므로 여기서 통과시키지 않으면 첫 레포로 되돌아간다.
  if (preferred === ALL_REPOS) return ALL_REPOS;
  return repos.find((repo) => repo.path === preferred)?.path ?? repos[0]?.path;
}

/** 이슈 웹 페이지 주소 — 번호를 눌러 GitHub에서 원문을 열 때 쓴다. */
export function issueUrl(ownerRepo: string, number: number): string {
  return `https://github.com/${ownerRepo}/issues/${number}`;
}

/**
 * '전체' 보기를 가리키는 선택값 — 레포 경로 자리에 들어가되 어떤 경로와도 겹치지 않는다.
 *
 * 별도의 boolean을 두지 않는 이유: 선택 상태가 두 군데로 갈라지면 "전체이면서 /work/app"
 * 같은 불가능한 조합이 표현된다. 경로 하나가 선택 전부를 나타내야 그 상태가 생기지 않는다.
 */
export const ALL_REPOS = "\u0000all";

/** 섹션이 그리는 상태. `loading`은 아직 판정 전 — 백엔드가 돌려주는 값이 아니다. */
export type SectionStatus = RepoIssues["status"] | "loading";

/**
 * 목록 한 줄. 이슈만이 아니라 **어느 레포의 것인지**를 함께 든다 — 전체 보기에서는 행마다
 * 레포가 다르므로, 태스크 생성·삭제·GitHub 열기가 모두 이 값을 보고 제 레포로 가야 한다.
 */
export interface IssueRow {
  repoPath: string;
  ownerRepo: string | null;
  issue: GhIssue;
}

const rowsOf = (repo: GhRepo, results: Map<string, RepoIssues>): IssueRow[] => {
  const result = results.get(repo.path);
  if (!result || result.status !== "ready") return [];
  return result.issues.map((issue) => ({
    repoPath: repo.path,
    ownerRepo: result.ownerRepo ?? repo.owner_repo,
    issue,
  }));
};

/**
 * 화면에 낼 줄들. 레포 하나를 고른 상태면 그 레포의 목록을 gh가 준 순서 그대로,
 * `ALL_REPOS`면 후보 전부를 합쳐 **최근 갱신순**으로 낸다.
 *
 * 합칠 때 정렬을 다시 하는 이유: 레포별 순서를 이어 붙이면 뒤쪽 레포의 오늘 이슈가 앞쪽
 * 레포의 반년 전 이슈보다 아래로 간다. 한 목록으로 보는 의미가 거기서 사라진다.
 */
export function issueRows(
  repos: GhRepo[],
  results: Map<string, RepoIssues>,
  selected: string | undefined,
): IssueRow[] {
  if (selected !== ALL_REPOS) {
    const repo = repos.find((candidate) => candidate.path === selected);
    return repo ? rowsOf(repo, results) : [];
  }
  return repos
    .flatMap((repo) => rowsOf(repo, results))
    .sort((a, b) => Date.parse(b.issue.updated_at) - Date.parse(a.issue.updated_at));
}

/**
 * 전체 보기의 상태 — 하나라도 목록을 받았으면 `ready`다.
 *
 * 레포 하나가 실패했다고 나머지 레포의 이슈를 감추면, 볼 수 있는 것을 못 보게 된다.
 * 반대로 전부 실패했을 때는 사용자가 고칠 수 있는 사정(gh 부재)을 조회 실패보다 앞세운다.
 */
export function mergedStatus(repos: GhRepo[], results: Map<string, RepoIssues>): SectionStatus {
  const statuses = repos.map((repo) => results.get(repo.path)?.status);
  if (statuses.some((status) => status === "ready")) return "ready";
  if (statuses.some((status) => status === "unavailable")) return "unavailable";
  if (statuses.some((status) => status === "error")) return "error";
  return "loading";
}

/**
 * 삭제된 이슈 한 건을 뺀 결과 — 재조회 없이 화면을 맞춘다.
 *
 * 지운 뒤 목록을 다시 부르면 후보 전부를 또 조회하게 되고(레포당 gh 한 번), 그동안 목록이
 * 통째로 흔들린다. 지워진 것은 우리가 방금 지운 그 한 건뿐이므로 그것만 빼는 것으로 족하다.
 */
export function withoutIssue(
  results: Map<string, RepoIssues>,
  repoPath: string,
  number: number,
): Map<string, RepoIssues> {
  const result = results.get(repoPath);
  if (!result) return results;
  const next = new Map(results);
  next.set(repoPath, {
    ...result,
    issues: result.issues.filter((issue) => issue.number !== number),
  });
  return next;
}
