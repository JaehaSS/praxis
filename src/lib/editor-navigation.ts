export interface NavigationLocation {
  path: string;
  groupId: string;
  line: number;
  column: number;
  scrollTop: number;
  scrollLeft: number;
}

export interface NavigationState {
  entries: NavigationLocation[];
  index: number;
}

const sameLocation = (left: NavigationLocation, right: NavigationLocation) =>
  left.path === right.path &&
  left.groupId === right.groupId &&
  left.line === right.line &&
  left.column === right.column;

export function emptyNavigation(): NavigationState {
  return { entries: [], index: -1 };
}

export function pushNavigation(state: NavigationState, location: NavigationLocation): NavigationState {
  const entries = state.entries.slice(0, state.index + 1);
  const previous = entries[entries.length - 1];
  if (previous && sameLocation(previous, location)) {
    return { entries: [...entries.slice(0, -1), location], index: entries.length - 1 };
  }
  const next = [...entries, location].slice(-100);
  return { entries: next, index: next.length - 1 };
}

export function moveNavigation(state: NavigationState, direction: -1 | 1): NavigationState {
  if (state.entries.length === 0) return state;
  const index = Math.max(0, Math.min(state.entries.length - 1, state.index + direction));
  return { entries: state.entries, index };
}

export function currentNavigation(state: NavigationState): NavigationLocation | null {
  return state.entries[state.index] ?? null;
}
