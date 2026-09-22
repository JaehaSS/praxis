import type { Task } from "./ipc";
import type { HostId } from "./transport";

const KEY = "praxis-task-list-cache-v1";
const MAX_ROWS = 200;
const MAX_AGE_SECONDS = 30 * 24 * 60 * 60;

interface CachedTask {
  id: number;
  repo: string;
  title: string;
  state: string;
  updated_at: number;
}

type Cache = Record<string, CachedTask[]>;

function isCachedTask(value: unknown): value is CachedTask {
  if (typeof value !== "object" || value === null) return false;
  const task = value as Record<string, unknown>;
  return typeof task.id === "number" && typeof task.repo === "string" &&
    typeof task.title === "string" && typeof task.state === "string" && typeof task.updated_at === "number";
}

function read(): Cache {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) || "{}");
    if (typeof value !== "object" || value === null) return {};
    return Object.fromEntries(Object.entries(value).flatMap(([host, tasks]) =>
      Array.isArray(tasks) ? [[host, tasks.filter(isCachedTask).slice(0, MAX_ROWS)]] : [],
    ));
  } catch {
    return {};
  }
}

function write(cache: Cache): void {
  try { localStorage.setItem(KEY, JSON.stringify(cache)); } catch { /* cache is optional */ }
}

function title(task: Task): string {
  return (task.instruction || task.branch).replace(/\s+/g, " ").slice(0, 120);
}

export class TaskListCache {
  private cache = read();

  save(host: HostId, tasks: Task[]): void {
    const oldest = Math.floor(Date.now() / 1000) - MAX_AGE_SECONDS;
    this.cache[host] = tasks
      .filter((task) => task.updated_at >= oldest)
      .slice(0, MAX_ROWS)
      .map((task) => ({ id: task.id, repo: task.repo, title: title(task), state: task.state, updated_at: task.updated_at }));
    write(this.cache);
  }

  load(host: HostId): Task[] {
    const oldest = Math.floor(Date.now() / 1000) - MAX_AGE_SECONDS;
    return (this.cache[host] ?? [])
      .filter((task) => task.updated_at >= oldest)
      .map((task) => ({
        host, id: task.id, repo: task.repo, instruction: task.title, branch: "", base: "", worktree_path: "",
        state: task.state, created_at: task.updated_at, updated_at: task.updated_at, mode: "conversation", stale: true,
      }));
  }
}

export const taskListCache = new TaskListCache();
