import type { DesignBoundingRect } from "./types";

export interface PreviewRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const MIN_SIZE = 1;
const SUBPIXEL_SCALE = 2;

function finite(value: number): boolean {
  return Number.isFinite(value);
}

function normalize(value: number): number {
  return Math.round(value * SUBPIXEL_SCALE) / SUBPIXEL_SCALE;
}

export function previewBoundsFromRect(rect: PreviewRect): DesignBoundingRect | null {
  if (![rect.x, rect.y, rect.width, rect.height].every(finite)) return null;
  if (rect.width < MIN_SIZE || rect.height < MIN_SIZE) return null;
  return {
    x: normalize(rect.x),
    y: normalize(rect.y),
    width: normalize(rect.width),
    height: normalize(rect.height),
  };
}

export function previewBoundsChanged(
  current: DesignBoundingRect | null,
  next: DesignBoundingRect,
): boolean {
  if (current == null) return true;
  return (
    current.x !== next.x ||
    current.y !== next.y ||
    current.width !== next.width ||
    current.height !== next.height
  );
}
