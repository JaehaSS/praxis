// 계획 캘린더 순수 로직 — 렌더와 IPC에서 분리해 단위 테스트한다 (설계 0023).
//
// 날짜 기준은 **KST 고정**이다. `day_items.day`가 KST로 찍히므로(commands.rs resolve_day)
// 프론트가 로컬 타임존으로 "오늘"을 계산하면 UTC 15:00~24:00 구간에서 하루 어긋나
// 빈 목록이 뜬다 (DR-4).

import { progress, type DayItem, type Progress } from "../today-items";

/** `today::day::KST_OFFSET_SECS`의 거울. */
const KST_OFFSET_MS = 32400 * 1000;

export interface CalendarCell {
  /** 'YYYY-MM-DD' */
  day: string;
  /** 1~31 */
  date: number;
  /** 표시 중인 달이 아닌 앞뒤 달의 날짜 */
  outside: boolean;
}

export interface DaySummary extends Progress {
  done: number;
  open: number;
  dropped: number;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** KST 기준 오늘 'YYYY-MM-DD'. */
export function kstToday(nowMs: number): string {
  return new Date(nowMs + KST_OFFSET_MS).toISOString().slice(0, 10);
}

/** 'YYYY-MM-DD' → 'YYYY-MM'. */
export function monthOf(day: string): string {
  return day.slice(0, 7);
}

/** 'YYYY-MM' → delta달 뒤. 음수면 이전 달. */
export function addMonth(month: string, delta: number): string {
  const [y, m] = month.split("-").map(Number);
  const shifted = new Date(Date.UTC(y, m - 1 + delta, 1));
  return `${shifted.getUTCFullYear()}-${pad(shifted.getUTCMonth() + 1)}`;
}

/** 그 달의 첫날·마지막날. `today_range`에 그대로 넘긴다(경계 포함). */
export function monthBounds(month: string): { from: string; to: string } {
  const [y, m] = month.split("-").map(Number);
  const last = new Date(Date.UTC(y, m, 0)).getUTCDate();
  return { from: `${month}-01`, to: `${month}-${pad(last)}` };
}

/**
 * 일요일 정렬 6주 격자. 주 수를 달마다 바꾸면 섹션 높이가 출렁이므로 6주로 고정하고,
 * 남는 칸은 앞뒤 달 날짜(`outside`)로 채운다.
 *
 * UTC로 계산한다 — 격자는 달력상의 날짜 배열일 뿐이라 타임존과 무관해야 한다.
 */
export function monthGrid(month: string): CalendarCell[][] {
  const [y, m] = month.split("-").map(Number);
  const first = new Date(Date.UTC(y, m - 1, 1));
  const start = new Date(Date.UTC(y, m - 1, 1 - first.getUTCDay()));

  const weeks: CalendarCell[][] = [];
  for (let w = 0; w < 6; w++) {
    const week: CalendarCell[] = [];
    for (let d = 0; d < 7; d++) {
      const at = new Date(start);
      at.setUTCDate(start.getUTCDate() + w * 7 + d);
      const day = at.toISOString().slice(0, 10);
      week.push({ day, date: at.getUTCDate(), outside: monthOf(day) !== month });
    }
    weeks.push(week);
  }
  return weeks;
}

/**
 * 날짜별 집계. `dropped`는 진행률 분모에서 빠진다 — `progress()`의 규약을 그대로 쓴다
 * (접은 계획이 달성률을 깎으면 항목을 접지 않고 방치하게 된다).
 */
export function summarize(items: DayItem[]): Map<string, DaySummary> {
  const byDay = new Map<string, DayItem[]>();
  for (const item of items) {
    const bucket = byDay.get(item.day);
    if (bucket) bucket.push(item);
    else byDay.set(item.day, [item]);
  }

  const out = new Map<string, DaySummary>();
  for (const [day, group] of byDay) {
    out.set(day, {
      ...progress(group),
      done: group.filter((i) => i.status === "done").length,
      open: group.filter((i) => i.status === "open").length,
      dropped: group.filter((i) => i.status === "dropped").length,
    });
  }
  return out;
}

/** 그 날의 항목만, 표시 순서(position)대로. */
export function itemsOf(items: DayItem[], day: string): DayItem[] {
  return items.filter((i) => i.day === day);
}

/** '2026-08-07' → '8월 7일'. 선택일 헤더용. */
export function fmtDayLabel(day: string): string {
  const [, m, d] = day.split("-").map(Number);
  return `${m}월 ${d}일`;
}
