// 커스텀 테마 파일(schema v1)의 검증·직렬화, 레지스트리 채우기, 부팅 캐시.
// Tauri invoke와 localStorage 접근은 전부 여기가 소유한다 — themes.ts는 동기·무IO로 남아야
// 보조 창과 첫 프레임에서도 안전하게 로드된다.

import { invoke } from "@tauri-apps/api/core";
import {
  applyTheme,
  deriveTheme,
  getActiveTheme,
  isCustomThemeId,
  registerCustomThemes,
  subscribeTheme,
  DEFAULT_THEME_ID,
  DRAFT_THEME_ID,
  type DerivedKey,
  type PaletteKey,
  type SyntaxPalette,
  type Theme,
  type ThemeKind,
  type ThemeSpec,
} from "./themes";

const HEX = /^#[0-9a-f]{6}$/i;
/** Rust `theme_store::valid_id`와 같은 규칙 — 여기를 통과한 spec은 항상 저장 가능하다. */
const ID = /^custom-[a-z0-9-]+$/;
const ID_MAX_LEN = 64;

export interface CustomThemeSpec {
  schema: 1;
  id: string;
  label: string;
  kind: "dark" | "light";
  basedOn?: string;
  palette: Record<PaletteKey, string>;
  /**
   * syntax가 (ThemeSpec과 달리) 필수인 이유: 커스텀은 에디터가 항상 10색을 제공한다
   * (DEFAULT_SYNTAX 시드) — 내장 상속 예외는 빌트인 Praxis 2종뿐이다(플랜 0049 DR-1).
   */
  syntax: SyntaxPalette;
  overrides?: Partial<Record<DerivedKey, string>>;
}

// 검증용 런타임 키 목록. `satisfies`가 themes.ts의 키 추가·삭제를 컴파일 타임에 잡는다.
// 편집 폼(ThemeEditor)의 칸 순서도 이 목록이 정본이다 — 검증과 UI가 갈라지지 않는다.
export const PALETTE_KEYS = Object.keys({
  bg: 1, surface: 1, raised: 1, border: 1, borderStrong: 1, primary: 1, text: 1, text2: 1,
  textMuted: 1, running: 1, awaiting: 1, question: 1, done: 1, failed: 1,
} satisfies Record<PaletteKey, 1>) as PaletteKey[];
export const SYNTAX_KEYS = Object.keys({
  keyword: 1, string: 1, number: 1, comment: 1, type: 1,
  func: 1, variable: 1, constant: 1, operator: 1, tag: 1,
} satisfies Record<keyof SyntaxPalette, 1>) as (keyof SyntaxPalette)[];
export const DERIVED_KEYS = Object.keys({
  primaryBright: 1, primaryHover: 1, addbg: 1, delbg: 1, addbgStrong: 1, delbgStrong: 1,
  dangerbg: 1, dangerborder: 1, empty: 1, scrollbarThumb: 1, scrollbarThumbHover: 1,
  scrollbarThumbActive: 1, termCursor: 1, termSelection: 1,
} satisfies Record<DerivedKey, 1>) as DerivedKey[];
const DERIVED_KEY_SET = new Set<string>(DERIVED_KEYS);

function reject(msg: string): null {
  console.warn(`커스텀 테마: ${msg}`);
  return null;
}

/** 필수 키가 전부 있고 값이 #rrggbb인지. 모르는 키는 버린다(전방 호환). */
function colorMap<K extends string>(value: unknown, keys: readonly K[], id: string, field: string): Record<K, string> | null {
  if (!value || typeof value !== "object") return reject(`${id}: ${field}가 객체가 아닙니다`);
  const src = value as Record<string, unknown>;
  const out = {} as Record<K, string>;
  for (const k of keys) {
    const hex = src[k];
    if (typeof hex !== "string" || !HEX.test(hex)) {
      return reject(`${id}: ${field}.${k}가 #rrggbb가 아닙니다`);
    }
    out[k] = hex;
  }
  return out;
}

/** 없으면 undefined, 깨졌으면 null(=파일 거부). 모르는 키는 버린다. */
function parseOverrides(value: unknown, id: string): Partial<Record<DerivedKey, string>> | null | undefined {
  if (value === undefined) return undefined;
  if (!value || typeof value !== "object") return reject(`${id}: overrides가 객체가 아닙니다`);
  const out: Partial<Record<DerivedKey, string>> = {};
  for (const [k, hex] of Object.entries(value as Record<string, unknown>)) {
    if (!DERIVED_KEY_SET.has(k)) continue;
    // 에디터는 깨진 override를 만들지 않는다 — 손으로 고친 값이므로 조용히 버리지 않고 거부한다.
    if (typeof hex !== "string" || !HEX.test(hex)) {
      return reject(`${id}: overrides.${k}가 #rrggbb가 아닙니다`);
    }
    out[k as DerivedKey] = hex;
  }
  return out;
}

/** 검증 실패는 null + 경고 — 한 파일이 깨져도 나머지 테마는 산다. */
export function parseThemeSpec(raw: string): CustomThemeSpec | null {
  let v: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return reject("최상위가 객체가 아닙니다");
    v = parsed as Record<string, unknown>;
  } catch {
    return reject("JSON 파싱 실패");
  }
  if (v.schema !== 1) return reject(`schema ${String(v.schema)}는 지원하지 않습니다`);
  const id = typeof v.id === "string" ? v.id : "";
  if (!ID.test(id) || id.length > ID_MAX_LEN) return reject(`잘못된 id: ${id}`);
  // 드래프트 id는 편집 중에만 존재하는 임시 이름이다 — 파일이 그 이름을 차지하면 편집 화면의
  // 롤백(활성이 드래프트면 되돌린다)이 방금 저장한 테마를 지운다.
  if (id === DRAFT_THEME_ID) return reject(`${id}는 편집 중 예약된 id입니다`);
  if (typeof v.label !== "string" || !v.label) return reject(`${id}: label이 없습니다`);
  if (v.kind !== "dark" && v.kind !== "light") return reject(`${id}: kind가 dark|light가 아닙니다`);

  const palette = colorMap(v.palette, PALETTE_KEYS, id, "palette");
  const syntax = colorMap(v.syntax, SYNTAX_KEYS, id, "syntax");
  const overrides = parseOverrides(v.overrides, id);
  if (!palette || !syntax || overrides === null) return null;

  return {
    schema: 1,
    id,
    label: v.label,
    kind: v.kind,
    ...(typeof v.basedOn === "string" ? { basedOn: v.basedOn } : {}),
    palette,
    syntax,
    ...(overrides ? { overrides } : {}),
  };
}

export function serializeThemeSpec(spec: CustomThemeSpec): string {
  return JSON.stringify(spec, null, 2);
}

/** 커스텀 spec → derive 입력. blurb는 schema v1에 없어 고정 문구를 쓴다. */
export function toThemeSpec(spec: CustomThemeSpec): ThemeSpec {
  return {
    id: spec.id,
    label: spec.label,
    blurb: "커스텀 테마",
    kind: spec.kind,
    palette: spec.palette,
    syntax: spec.syntax,
    overrides: spec.overrides,
  };
}

// ─── 편집 진입 (복제·id 부여) ──────────────────────────────────────────────

/**
 * 복제 시 채울 구문색. Praxis 빌트인 2종은 syntax가 없으므로(플랜 0049 DR-1) 시드가 필요하다.
 * VS Code Dark+/Light+ 근사 — 편집 시작점일 뿐이라 정밀성보다 익숙함이 기준이다.
 */
export const DEFAULT_SYNTAX: Record<ThemeKind, SyntaxPalette> = {
  dark: {
    keyword: "#569cd6", string: "#ce9178", number: "#b5cea8", comment: "#6a9955",
    type: "#4ec9b0", func: "#dcdcaa", variable: "#9cdcfe", constant: "#4fc1ff",
    operator: "#d4d4d4", tag: "#569cd6",
  },
  light: {
    keyword: "#0000ff", string: "#a31515", number: "#098658", comment: "#008000",
    type: "#267f99", func: "#795e26", variable: "#001080", constant: "#0070c1",
    operator: "#000000", tag: "#800000",
  },
};

function clampId(id: string): string {
  return (id.length <= ID_MAX_LEN ? id : id.slice(0, ID_MAX_LEN)).replace(/-+$/, "");
}

/**
 * label → `custom-<slug>`. id는 파일명이자 Rust 검증 대상이라 `[a-z0-9-]`만 남는다 —
 * 한글처럼 옮길 수 없는 문자는 떨어져 나가고, 전부 떨어지면 `custom-theme`으로 간다
 * (중복은 `uniqueCustomId`가 번호로 가른다. 라벨은 사람이 읽는 이름이라 그대로 살아 있다).
 */
export function customIdFromLabel(label: string): string {
  const slug = label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return clampId(`custom-${slug || "theme"}`);
}

/** 이미 쓰는 id면 `-2`, `-3`… 을 붙인다. 길이 상한을 넘지 않도록 앞을 자른다. */
export function uniqueCustomId(id: string, taken: Iterable<string>): string {
  // 드래프트 id는 항상 예약이다 — 라벨 "Draft"가 우연히 그 이름으로 슬러그되면 안 된다.
  const used = new Set(taken).add(DRAFT_THEME_ID);
  if (!used.has(id)) return id;
  for (let n = 2; n < 1000; n++) {
    const suffix = `-${n}`;
    const next = clampId(id.slice(0, ID_MAX_LEN - suffix.length)) + suffix;
    if (!used.has(next)) return next;
  }
  return `custom-${Date.now()}`;
}

/**
 * Theme(빌트인 포함) → 편집 가능한 spec. 팔레트는 AA 보정을 마친 토큰에서 읽는다 —
 * 화면에 보이던 색이 곧 편집 시작값이다. overrides는 비워 파생에 맡긴다(빈 값 = 자동).
 */
export function specFromTheme(theme: Theme, label: string, taken: Iterable<string>): CustomThemeSpec {
  const palette = {} as Record<PaletteKey, string>;
  for (const k of PALETTE_KEYS) palette[k] = theme.tokens[k];
  return {
    schema: 1,
    id: uniqueCustomId(customIdFromLabel(label), taken),
    label,
    kind: theme.kind,
    basedOn: theme.id,
    palette,
    syntax: { ...(theme.syntax ?? DEFAULT_SYNTAX[theme.kind]) },
  };
}

/** 활성 커스텀 테마의 **spec 원문**. 파생 토큰이 아니다 — 플랜 0049 DR-4. */
const CACHE_KEY = "praxis-theme-cache";

let specs = new Map<string, CustomThemeSpec>();

function register(list: CustomThemeSpec[]): void {
  specs = new Map(list.map((s) => [s.id, s]));
  registerCustomThemes(list.map((s) => deriveTheme(toThemeSpec(s))));
}

function readCache(): string {
  try {
    return localStorage.getItem(CACHE_KEY) ?? "";
  } catch {
    return "";
  }
}

/** 캐시가 가리키던 테마가 더는 없을 때. 남겨 두면 다음 부팅이 같은 좀비를 되살린다. */
function clearCache(): void {
  try {
    localStorage.removeItem(CACHE_KEY);
  } catch {
    // 시크릿 모드 등 — 애초에 쓰이지 않았으니 지울 것도 없다.
  }
}

let cacheSyncing = false;

/**
 * 활성 테마가 커스텀이면 그 spec 원문을 캐시에 유지한다. themes.ts에 IO를 넣지 않으려고
 * applyTheme 본문 대신 구독으로 붙였다 — registerCustomThemes도 같은 구독을 발화하므로
 * "파일 정본 로드 → 캐시 갱신"까지 한 경로로 덮인다.
 */
function ensureCacheSync(): void {
  if (cacheSyncing) return;
  cacheSyncing = true;
  subscribeTheme(() => {
    const spec = specs.get(getActiveTheme().id);
    if (!spec) return;
    try {
      localStorage.setItem(CACHE_KEY, serializeThemeSpec(spec));
    } catch {
      // 시크릿 모드 등 — 다음 부팅에 FOUC가 한 프레임 보일 뿐이다.
    }
  });
}

/** 보조 창은 캐시만 읽어 첫 프레임을 맞추며, 파일 IPC 정본 조회는 메인 창만 수행한다. */
export function bootCachedCustomTheme(activeId: string): void {
  ensureCacheSync();
  const raw = isCustomThemeId(activeId) ? readCache() : "";
  const cached = raw ? parseThemeSpec(raw) : null;
  // 캐시가 다른 테마의 것이면 쓰지 않는다 — FOUC를 막을 대상은 활성 테마뿐이다.
  if (cached?.id === activeId) register([cached]);
}

/** 부팅: 캐시 spec을 동기 등록(FOUC 차단)한 뒤 파일 정본을 비동기로 로드해 재등록·재적용. */
export function bootCustomThemes(activeId: string): void {
  bootCachedCustomTheme(activeId);
  // 부팅의 백그라운드 경로에는 실패를 드러낼 화면이 없다 — 여기서만 삼킨다.
  void loadCustomThemes(activeId).catch((e: unknown) => {
    console.warn("커스텀 테마 목록을 읽지 못했습니다", e);
  });
}

/**
 * 파일 정본으로 레지스트리를 채운다. 깨진 파일은 건너뛴다.
 *
 * `theme_list` 실패는 **던진다** — 저장·삭제 뒤의 재로드가 조용히 끝나면 편집 화면은
 * 성공한 것처럼 닫힌다. 백그라운드 부팅만 `bootCustomThemes`가 잡는다.
 */
export async function loadCustomThemes(desiredId?: string): Promise<void> {
  ensureCacheSync();
  const before = getActiveTheme().id;
  const raws = await invoke<string[]>("theme_list");
  register(raws.map((raw) => parseThemeSpec(raw)).filter((s): s is CustomThemeSpec => s !== null));
  // 로드 중에 사용자가 테마를 바꿨으면 그 선택이 이긴다 — 뒤늦은 재적용이 덮지 않는다.
  if (getActiveTheme().id !== before) return;
  // 캐시 미스로 기본 테마에 떨어졌거나 파일 정본이 캐시와 다를 수 있다 — 정본으로 다시 적용한다.
  const target = desiredId ?? before;
  if (specs.has(target)) {
    applyTheme(target);
    return;
  }
  // 파일이 밖에서 지워진 커스텀 테마다. 캐시를 남기면 다음 부팅이 다시 살려내 영영 반복된다.
  if (!isCustomThemeId(target)) return;
  applyTheme(DEFAULT_THEME_ID);
  clearCache();
}

/** 등록된 커스텀 테마의 **원본 spec** — 편집·복제 진입이 파생 토큰이 아니라 여기서 시작한다. */
export function customSpecs(): CustomThemeSpec[] {
  return [...specs.values()];
}

/**
 * 저장 → 파일 정본 재로드. 실패는 호출자가 문구로 드러낸다(설정 화면 인라인).
 *
 * `apply: false`는 목록에만 더한다 — 가져오기가 남의 테마로 화면을 말없이 갈아치우지 않게.
 */
export async function saveCustomTheme(
  spec: CustomThemeSpec,
  opts: { apply?: boolean } = {},
): Promise<void> {
  await invoke("theme_save", { id: spec.id, json: serializeThemeSpec(spec) });
  await loadCustomThemes(opts.apply === false ? undefined : spec.id);
}

/** 삭제 → 재로드. 활성 테마를 지울 때는 **호출 전에** 다른 테마로 옮겨 둔다. */
export async function deleteCustomTheme(id: string): Promise<void> {
  await invoke("theme_delete", { id });
  await loadCustomThemes();
}

export async function exportCustomTheme(id: string, dest: string): Promise<void> {
  await invoke("theme_export", { id, dest });
}

/** 외부 파일 원문 읽기. 검증과 id 충돌 해소는 호출자 몫이다. */
export function readThemeFile(src: string): Promise<string> {
  return invoke<string>("theme_import", { src });
}
