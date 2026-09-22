// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import {
  bootCachedCustomTheme,
  bootCustomThemes,
  customIdFromLabel,
  DEFAULT_SYNTAX,
  loadCustomThemes,
  parseThemeSpec,
  saveCustomTheme,
  serializeThemeSpec,
  specFromTheme,
  toThemeSpec,
  uniqueCustomId,
  type CustomThemeSpec,
} from "./theme-files";
import {
  allThemes,
  applyDraft,
  applyTheme,
  DEFAULT_THEME_ID,
  getActiveTheme,
  getTheme,
  loadThemeId,
  registerCustomThemes,
  THEMES,
} from "./themes";

const CACHE_KEY = "praxis-theme-cache";

const SPEC: CustomThemeSpec = {
  schema: 1,
  id: "custom-dusk",
  label: "Dusk",
  kind: "dark",
  basedOn: "praxis-dark",
  palette: {
    bg: "#0d0d0d", surface: "#161616", raised: "#1f1f1f", border: "#2a2a2a",
    borderStrong: "#3f3f46", primary: "#14b8a6", text: "#f4f4f5", text2: "#a1a1aa",
    textMuted: "#71717a", running: "#60a5fa", awaiting: "#fbbf24", question: "#c084fc",
    done: "#4ade80", failed: "#f87171",
  },
  syntax: {
    keyword: "#569cd6", string: "#ce9178", number: "#b5cea8", comment: "#6a9955",
    type: "#4ec9b0", func: "#dcdcaa", variable: "#9cdcfe", constant: "#4fc1ff",
    operator: "#d4d4d4", tag: "#569cd6",
  },
  overrides: { addbg: "#071a0f" },
};

/** SPEC을 부분 수정한 원문. 검증 실패 경로는 JSON 문자열로만 표현된다. */
function rawWith(patch: Record<string, unknown>): string {
  return JSON.stringify({ ...SPEC, ...patch });
}

beforeEach(() => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  // 호출 이력까지 지운다 — 남겨 두면 "저장이 theme_save를 불렀는가" 같은 단언이 앞 테스트의
  // 호출로 통과해 검증력을 잃는다.
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue([]);
  localStorage.clear();
  registerCustomThemes([]);
  applyTheme(DEFAULT_THEME_ID);
  // 캐시만 한 번 더 지운다. 앞 테스트가 커스텀을 활성으로 두고 끝났으면 registerCustomThemes가
  // 발화하는 구독이 (specs는 아직 그 spec을 들고 있어) 방금 지운 캐시를 다시 써 넣는다.
  localStorage.removeItem(CACHE_KEY);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("parseThemeSpec", () => {
  it("정상 spec을 통과시킨다", () => {
    expect(parseThemeSpec(serializeThemeSpec(SPEC))).toEqual(SPEC);
  });

  it("palette 키가 빠지면 거부한다", () => {
    const { bg: _bg, ...rest } = SPEC.palette;
    expect(parseThemeSpec(rawWith({ palette: rest }))).toBeNull();
    const { tag: _tag, ...syntax } = SPEC.syntax;
    expect(parseThemeSpec(rawWith({ syntax }))).toBeNull();
  });

  it("hex 형식이 아니면 거부한다", () => {
    expect(parseThemeSpec(rawWith({ palette: { ...SPEC.palette, bg: "0d0d0d" } }))).toBeNull();
    expect(parseThemeSpec(rawWith({ palette: { ...SPEC.palette, bg: "#fff" } }))).toBeNull();
    expect(parseThemeSpec(rawWith({ syntax: { ...SPEC.syntax, keyword: "blue" } }))).toBeNull();
    expect(parseThemeSpec(rawWith({ overrides: { addbg: "rgb(0,0,0)" } }))).toBeNull();
  });

  it("모르는 키는 무시한다 (전방 호환)", () => {
    const raw = rawWith({
      future: "v2 필드",
      palette: { ...SPEC.palette, neon: "#ff00ff" },
      syntax: { ...SPEC.syntax, macro: "#ff00ff" },
      overrides: { ...SPEC.overrides, ghost: "#ff00ff" },
    });
    expect(parseThemeSpec(raw)).toEqual(SPEC);
  });

  it("id 규약을 벗어나면 거부한다", () => {
    // Rust `theme_store::valid_id`와 같은 규칙이다 — 여기를 통과한 spec은 항상 저장 가능해야
    // 하므로, 접두사·문자 집합·길이 중 하나라도 어긋나면 파일을 받지 않는다.
    expect(parseThemeSpec(rawWith({ id: "dusk" }))).toBeNull();
    expect(parseThemeSpec(rawWith({ id: "custom-" }))).toBeNull();
    expect(parseThemeSpec(rawWith({ id: "custom-Dusk" }))).toBeNull();
    expect(parseThemeSpec(rawWith({ id: "custom-../evil" }))).toBeNull();
    expect(parseThemeSpec(rawWith({ id: `custom-${"a".repeat(58)}` }))).toBeNull();
    // 경계 바로 아래(64자)는 통과한다 — 상한이 한 칸 밀리지 않았는지 같이 못박는다.
    expect(parseThemeSpec(rawWith({ id: `custom-${"a".repeat(57)}` }))).not.toBeNull();
  });

  it("드래프트 id를 쓴 파일은 거부한다", () => {
    // 파일이 이 이름을 차지하면 편집 화면의 롤백(활성이 드래프트면 되돌린다)이
    // 방금 저장한 테마를 지운다.
    expect(parseThemeSpec(rawWith({ id: "custom-draft" }))).toBeNull();
  });

  it("schema가 1이 아니면 거부한다", () => {
    expect(parseThemeSpec(rawWith({ schema: 2 }))).toBeNull();
    expect(parseThemeSpec(JSON.stringify({ ...SPEC, schema: undefined }))).toBeNull();
  });

  it("직렬화 라운드트립이 값을 보존한다", () => {
    const once = parseThemeSpec(serializeThemeSpec(SPEC));
    expect(once).not.toBeNull();
    expect(parseThemeSpec(serializeThemeSpec(once as CustomThemeSpec))).toEqual(SPEC);
  });
});

describe("id 부여", () => {
  it("label을 slug로 옮긴다", () => {
    expect(customIdFromLabel("My Dusk")).toBe("custom-my-dusk");
    expect(customIdFromLabel("  Nord — 2!  ")).toBe("custom-nord-2");
  });

  it("옮길 수 없는 문자만 있으면 기본 이름으로 떨어진다", () => {
    // 한글은 파일명·Rust 검증(`custom-[a-z0-9-]+`)을 통과하지 못한다. 라벨은 그대로 살아 있고
    // 이름이 겹치는 문제는 uniqueCustomId가 번호로 가른다.
    expect(customIdFromLabel("황혼")).toBe("custom-theme");
    expect(customIdFromLabel("")).toBe("custom-theme");
  });

  it("64자 상한을 넘지 않고 꼬리 하이픈을 남기지 않는다", () => {
    const id = customIdFromLabel("a".repeat(80));
    expect(id.length).toBeLessThanOrEqual(64);
    expect(id.endsWith("-")).toBe(false);
  });

  it("이미 쓰는 id면 번호를 붙인다", () => {
    expect(uniqueCustomId("custom-dusk", [])).toBe("custom-dusk");
    expect(uniqueCustomId("custom-dusk", ["custom-dusk"])).toBe("custom-dusk-2");
    expect(uniqueCustomId("custom-dusk", ["custom-dusk", "custom-dusk-2"])).toBe("custom-dusk-3");
  });

  it("번호를 붙여도 상한을 넘지 않는다", () => {
    const long = customIdFromLabel("a".repeat(80));
    expect(uniqueCustomId(long, [long]).length).toBeLessThanOrEqual(64);
  });
});

describe("복제", () => {
  it("syntax가 없는 빌트인은 기본 구문색으로 시드한다 (플랜 0049 DR-1)", () => {
    const spec = specFromTheme(getTheme(DEFAULT_THEME_ID), "Praxis Dark 사본", []);

    expect(spec.syntax).toEqual(DEFAULT_SYNTAX.dark);
    expect(spec.basedOn).toBe(DEFAULT_THEME_ID);
    // "Praxis Dark 사본" → 옮길 수 없는 "사본"이 떨어지고 꼬리 하이픈도 남지 않는다.
    expect(spec.id).toBe("custom-praxis-dark");
  });

  it("팔레트는 화면에 보이던 값(AA 보정 후)을 그대로 가져온다", () => {
    const nord = getTheme("nord");
    const spec = specFromTheme(nord, "Nord 사본", []);

    expect(spec.palette.bg).toBe(nord.tokens.bg);
    expect(spec.palette.failed).toBe(nord.tokens.failed);
    expect(spec.syntax).toEqual(nord.syntax);
    // override는 비워 파생에 맡긴다 — 편집 화면 "고급"이 자동값을 placeholder로 보여준다.
    expect(spec.overrides).toBeUndefined();
    expect(parseThemeSpec(serializeThemeSpec(spec))).toEqual(spec);
  });

  it("이미 있는 커스텀 id를 덮지 않는다", () => {
    const spec = specFromTheme(getTheme("nord"), "Nord 사본", ["custom-nord"]);
    expect(spec.id).toBe("custom-nord-2");
  });
});

describe("드래프트 적용", () => {
  it("CSS 변수를 바꾸되 저장하거나 레지스트리에 남기지 않는다", () => {
    applyDraft({ ...toThemeSpec(SPEC), palette: { ...SPEC.palette, bg: "#123456" } });

    expect(document.documentElement.style.getPropertyValue("--c-bg")).toBe("#123456");
    expect(getActiveTheme().id).toBe("custom-draft");
    // 저장된 활성 테마는 진입 시점 그대로다 — 드래프트는 persist하지 않는다.
    expect(localStorage.getItem("praxis-theme")).toBe(DEFAULT_THEME_ID);
    expect(allThemes().some((t) => t.id === "custom-draft")).toBe(false);
  });

  it("취소는 이전 테마 적용 한 번으로 되돌아간다", () => {
    applyDraft(toThemeSpec(SPEC));
    applyTheme(DEFAULT_THEME_ID);

    expect(getActiveTheme().id).toBe(DEFAULT_THEME_ID);
    expect(document.documentElement.style.getPropertyValue("--c-bg")).toBe(
      getTheme(DEFAULT_THEME_ID).tokens.bg,
    );
  });
});

describe("레지스트리 로드", () => {
  it("깨진 파일은 건너뛰고 나머지를 등록한다", async () => {
    vi.mocked(invoke).mockResolvedValue([serializeThemeSpec(SPEC), "{ 깨진 파일"]);

    await loadCustomThemes();

    expect(allThemes()).toHaveLength(THEMES.length + 1);
    expect(getTheme("custom-dusk").label).toBe("Dusk");
  });

  it("부팅 시 캐시 spec을 동기 등록한다 (FOUC 차단)", async () => {
    localStorage.setItem(CACHE_KEY, serializeThemeSpec(SPEC));

    bootCustomThemes("custom-dusk");

    // await 없이 — 첫 프레임 전에 등록돼 있어야 한다.
    expect(getTheme("custom-dusk").label).toBe("Dusk");
    // 정본 로드가 끝나기 전에 테스트를 끝내면 그 재등록이 다음 테스트 도중 터진다.
    // 파일 목록이 비었으므로 캐시가 등록한 테마가 사라지는 것으로 완료를 안다.
    await vi.waitFor(() => expect(allThemes()).toHaveLength(THEMES.length));
  });

  it("보조 창 캐시 부팅은 파일 IPC를 호출하지 않는다", () => {
    localStorage.setItem(CACHE_KEY, serializeThemeSpec(SPEC));
    bootCachedCustomTheme("custom-dusk");
    expect(getTheme("custom-dusk").label).toBe("Dusk");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("파일이 사라진 활성 커스텀 테마는 기본 테마로 떨어지고 캐시도 지운다", async () => {
    localStorage.setItem(CACHE_KEY, serializeThemeSpec(SPEC));
    vi.mocked(invoke).mockResolvedValueOnce([serializeThemeSpec(SPEC)]);
    await loadCustomThemes("custom-dusk");
    expect(getActiveTheme().id).toBe("custom-dusk");

    // 밖에서 파일이 지워진 다음의 재로드. 캐시를 남기면 다음 부팅이 같은 좀비를 되살린다.
    await loadCustomThemes();

    expect(getActiveTheme().id).toBe(DEFAULT_THEME_ID);
    expect(localStorage.getItem(CACHE_KEY)).toBeNull();
  });

  it("목록 읽기가 실패하면 던지고 기존 레지스트리를 남긴다", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([serializeThemeSpec(SPEC)]);
    await loadCustomThemes();
    vi.mocked(invoke).mockRejectedValueOnce(new Error("권한 없음"));

    // 조용히 끝나면 편집 화면이 성공한 것처럼 닫힌다 — 호출자가 볼 수 있게 던져야 한다.
    await expect(loadCustomThemes()).rejects.toThrow("권한 없음");
    expect(getTheme("custom-dusk").label).toBe("Dusk");
  });
});

describe("부팅 캐시 기록", () => {
  /** invoke를 커맨드별로 가른다 — theme_save는 값이 없고 theme_list만 원문 목록을 준다. */
  function stubStore(list: string[]) {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "theme_list" ? list : undefined) as Promise<never>,
    );
  }

  it("커스텀 테마를 적용하면 spec 원문을 남긴다 (플랜 0049 DR-4)", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([serializeThemeSpec(SPEC)]);
    await loadCustomThemes();
    localStorage.clear();

    applyTheme("custom-dusk");

    // 파생 토큰이 아니라 spec 원문이다 — 다음 부팅이 이걸 그대로 parseThemeSpec에 넣는다.
    expect(parseThemeSpec(localStorage.getItem(CACHE_KEY) ?? "")).toEqual(SPEC);
  });

  it("빌트인과 드래프트는 캐시에 남기지 않는다", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([serializeThemeSpec(SPEC)]);
    await loadCustomThemes();
    localStorage.clear();

    applyTheme("nord");
    expect(localStorage.getItem(CACHE_KEY)).toBeNull();

    applyDraft(toThemeSpec(SPEC));
    expect(localStorage.getItem(CACHE_KEY)).toBeNull();
  });

  it("저장은 파일에 쓴 뒤 정본을 다시 읽어 적용한다", async () => {
    stubStore([serializeThemeSpec(SPEC)]);

    await saveCustomTheme(SPEC);

    expect(invoke).toHaveBeenCalledWith("theme_save", {
      id: "custom-dusk",
      json: serializeThemeSpec(SPEC),
    });
    expect(invoke).toHaveBeenCalledWith("theme_list");
    expect(getActiveTheme().id).toBe("custom-dusk");
  });

  it("apply: false는 목록에만 더하고 쓰던 테마를 두고 간다", async () => {
    stubStore([serializeThemeSpec(SPEC)]);

    await saveCustomTheme(SPEC, { apply: false });

    // 가져오기 경로 — 파일 하나 읽었다고 화면이 남의 테마로 바뀌면 놀란다.
    expect(getTheme("custom-dusk").label).toBe("Dusk");
    expect(getActiveTheme().id).toBe(DEFAULT_THEME_ID);
  });
});

describe("저장된 테마 id 읽기", () => {
  it("레지스트리가 비어도 custom- id는 통과시킨다", () => {
    // 부팅 순서상 파일 로드는 아직이다 — 여기서 기본 테마로 접으면 커스텀 테마는 영영 안 뜬다.
    localStorage.setItem("praxis-theme", "custom-dusk");
    expect(allThemes()).toHaveLength(THEMES.length);
    expect(loadThemeId()).toBe("custom-dusk");
  });

  it("모르는 빌트인 id는 기본 테마로 접는다", () => {
    localStorage.setItem("praxis-theme", "solarized-void");
    expect(loadThemeId()).toBe(DEFAULT_THEME_ID);
  });
});
