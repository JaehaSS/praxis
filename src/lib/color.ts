// 테마 파생 토큰(명도 변형·틴트·대비 보정)의 색 계산. DOM 의존 없는 순수 모듈.
//
// 테마는 팔레트 원색 14개만 선언하고 나머지 토큰은 여기서 만든다. 값을 손으로 적어두면
// 테마 8종 × 토큰 25개를 사람이 관리하게 되고, 하나가 어긋나도 눈으로는 안 보인다.

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export interface Hsl {
  h: number;
  s: number;
  l: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** `#rgb` / `#rrggbb` 모두 허용. 파싱 불가면 검정 — 테마 정의는 상수라 런타임에 깨질 값이 아니다. */
export function parseHex(hex: string): Rgb {
  const s = hex.trim().replace(/^#/, "");
  const full =
    s.length === 3
      ? s
          .split("")
          .map((c) => c + c)
          .join("")
      : s;
  if (!/^[0-9a-fA-F]{6}$/.test(full)) return { r: 0, g: 0, b: 0 };
  return {
    r: parseInt(full.slice(0, 2), 16),
    g: parseInt(full.slice(2, 4), 16),
    b: parseInt(full.slice(4, 6), 16),
  };
}

export function toHex({ r, g, b }: Rgb): string {
  const p = (v: number) =>
    clamp(Math.round(v), 0, 255)
      .toString(16)
      .padStart(2, "0");
  return `#${p(r)}${p(g)}${p(b)}`;
}

export function rgbToHsl({ r, g, b }: Rgb): Hsl {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };
  const s = d / (1 - Math.abs(2 * l - 1));
  let h: number;
  if (max === rn) h = ((gn - bn) / d) % 6;
  else if (max === gn) h = (bn - rn) / d + 2;
  else h = (rn - gn) / d + 4;
  h *= 60;
  return { h: h < 0 ? h + 360 : h, s, l };
}

export function hslToRgb({ h, s, l }: Hsl): Rgb {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const [r1, g1, b1] =
    hp < 1
      ? [c, x, 0]
      : hp < 2
        ? [x, c, 0]
        : hp < 3
          ? [0, c, x]
          : hp < 4
            ? [0, x, c]
            : hp < 5
              ? [x, 0, c]
              : [c, 0, x];
  const m = l - c / 2;
  return { r: (r1 + m) * 255, g: (g1 + m) * 255, b: (b1 + m) * 255 };
}

/** HSL 명도만 올린다(채도·색상 보존) — RGB 가산 방식은 색이 흰색으로 바래 팔레트 정체성을 잃는다. */
export function lighten(hex: string, amount: number): string {
  const hsl = rgbToHsl(parseHex(hex));
  return toHex(hslToRgb({ ...hsl, l: clamp(hsl.l + amount, 0, 1) }));
}

export function darken(hex: string, amount: number): string {
  const hsl = rgbToHsl(parseHex(hex));
  return toHex(hslToRgb({ ...hsl, l: clamp(hsl.l - amount, 0, 1) }));
}

/** a에서 b로 t(0~1)만큼. 선형 sRGB 보간이 아니라 단순 채널 보간 — 틴트 용도라 이 정도로 충분하다. */
export function mix(a: string, b: string, t: number): string {
  const ca = parseHex(a);
  const cb = parseHex(b);
  const k = clamp(t, 0, 1);
  return toHex({
    r: ca.r + (cb.r - ca.r) * k,
    g: ca.g + (cb.g - ca.g) * k,
    b: ca.b + (cb.b - ca.b) * k,
  });
}

/** CSS `rgba()` 문자열. Tailwind `rgb(from …)` 토큰에는 쓰지 말 것 — 알파 포함 값은 그 문법을 깨뜨린다. */
export function alpha(hex: string, a: number): string {
  const { r, g, b } = parseHex(hex);
  return `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${clamp(a, 0, 1)})`;
}

/** `#rrggbbaa`. xterm처럼 채널 8비트 hex만 확실히 파싱하는 소비자용. */
export function hexAlpha(hex: string, a: number): string {
  const aa = Math.round(clamp(a, 0, 1) * 255)
    .toString(16)
    .padStart(2, "0");
  return `${toHex(parseHex(hex))}${aa}`;
}

/** WCAG 2.1 상대 휘도. */
export function relativeLuminance(hex: string): number {
  const { r, g, b } = parseHex(hex);
  const ch = (v: number) => {
    const n = v / 255;
    return n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b);
}

/** WCAG 명암비 (1~21). */
export function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/**
 * fg가 bg 위에서 target 명암비를 못 채우면 명도를 옮겨 채운다.
 *
 * 팔레트 충실도와 가독성이 부딪히는 지점이다 — Nord·Gruvbox처럼 원래 저대비인 테마는
 * 상태색을 원본 그대로 쓰면 AA에 못 미친다. 색상·채도는 두고 명도만 최소한으로 움직여,
 * 테마의 성격은 남기되 읽을 수는 있게 한다. 흰/검까지 가도 target에 못 미치면 그 극단값.
 */
export function ensureContrast(fg: string, bg: string, target: number): string {
  if (contrastRatio(fg, bg) >= target) return fg;
  const hsl = rgbToHsl(parseHex(fg));
  // 방향은 하나뿐이다. 어두운 배경에서 색을 더 어둡게 하면 대비는 더 나빠진다.
  const towardLight = relativeLuminance(bg) < 0.5;
  const limit = towardLight ? 1 : 0;
  const at = (l: number) => toHex(hslToRgb({ ...hsl, l }));
  if (contrastRatio(at(limit), bg) < target) return at(limit);

  // 명도-대비는 이 방향에서 단조라 이진 탐색이 성립한다.
  let lo = hsl.l;
  let hi = limit;
  for (let i = 0; i < 24; i++) {
    const mid = (lo + hi) / 2;
    if (contrastRatio(at(mid), bg) >= target) hi = mid;
    else lo = mid;
  }
  return at(hi);
}
