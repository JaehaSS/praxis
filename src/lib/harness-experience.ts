import { invoke } from "@tauri-apps/api/core";
import { LOCAL_HOST, type HostId } from "./transport";

export type HarnessName = "workflow-harness" | "loop-engineering";
export type ReadError =
  | "not-found"
  | "permission-denied"
  | "unsafe-path"
  | "not-regular"
  | "oversized"
  | "invalid-utf8"
  | "changed"
  | "io-error";
export type SourceRef = {
  host: "local";
  harness: HarnessName;
  vendor: "claude" | "codex";
  scope: "project" | "global";
};
export type ProjectRef = { host: "local"; projectKey: string };
export type DocumentOwner =
  | { kind: "skill"; source: SourceRef }
  | { kind: "project"; project: ProjectRef };
export type DocumentDescriptor = {
  key: string;
  displayPath: string;
  relation: "harness-owned" | "project-reference";
};
export type SourceListing =
  | { source: SourceRef; state: "not-installed" | "empty" }
  | { source: SourceRef; state: "error"; error: ReadError }
  | {
      source: SourceRef;
      state: "ready";
      documents: DocumentDescriptor[];
      limited: boolean;
      inspectedEntries: number;
    };
export type ProjectListing =
  | { project: ProjectRef; state: "empty" }
  | { project: ProjectRef; state: "error"; error: ReadError }
  | {
      project: ProjectRef;
      state: "ready";
      documents: DocumentDescriptor[];
      limited: boolean;
      inspectedEntries: number;
    };
export type ExperienceDocument = {
  owner: DocumentOwner;
  documentKey: string;
  path: string;
  contentHash: string;
  observedAt: string;
  generated: boolean;
  sourceResolution: "known-set" | "unconfirmed" | "not-applicable";
  text: string;
};
export type ListResult =
  | { state: "unsupported"; reason: "remote-host" | "secure-read-unavailable" }
  | { state: "error"; error: "unregistered-project" | "invalid-request" }
  | {
      state: "ready";
      observedAt: string;
      sources: SourceListing[];
      project: ProjectListing;
    };
export type ReadResult =
  | { state: "ready"; document: ExperienceDocument }
  | { state: "unsupported"; reason: "remote-host" | "secure-read-unavailable" }
  | {
      state: "error";
      error: ReadError | "unregistered-project" | "invalid-request";
    };

export function loadExperiences(
  host: HostId,
  repo: string,
  harness: HarnessName,
): Promise<ListResult> {
  if (host !== LOCAL_HOST)
    return Promise.resolve({ state: "unsupported", reason: "remote-host" });
  return invoke<ListResult>("harness_experience_list", { repo, harness });
}

export function readExperience(
  host: HostId,
  repo: string,
  owner: DocumentOwner,
  documentKey: string,
): Promise<ReadResult> {
  if (host !== LOCAL_HOST)
    return Promise.resolve({ state: "unsupported", reason: "remote-host" });
  return invoke<ReadResult>("harness_experience_read", {
    repo,
    owner,
    documentKey,
  });
}
