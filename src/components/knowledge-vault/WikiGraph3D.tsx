import { useEffect, useRef, useState, type RefObject } from "react";
import type { WikiEdge, WikiPage } from "../../lib/wiki-workspace-ipc";
import type { FolderGroup } from "../../lib/wiki-folder-groups";
import type { Wiki3DView } from "./wiki-graph-3d";

interface Props {
  pages: WikiPage[]; edges: WikiEdge[]; selected: string | null; groups: readonly FolderGroup[];
  onSelect: (id: string) => void; onFailure: () => void;
  controller: RefObject<Wiki3DView | null>;
}

export function WikiGraph3D(props: Props) {
  const container = useRef<HTMLDivElement>(null);
  const latest = useRef(props); latest.current = props;
  const [loading, setLoading] = useState(true);
  const { controller } = props;
  useEffect(() => {
    let cancelled = false;
    let view: Wiki3DView | null = null;
    void import("./wiki-graph-3d").then(({ createWiki3D }) => {
      if (cancelled || !container.current) return;
      view = createWiki3D(container.current, id => { if (!cancelled) latest.current.onSelect(id); }, () => { if (!cancelled) latest.current.onFailure(); });
      controller.current = view;
      const { pages, edges, selected, groups } = latest.current;
      view.update(pages, edges, selected, groups);
      if (!cancelled) setLoading(false);
    }).catch(() => { if (!cancelled) latest.current.onFailure(); });
    return () => { cancelled = true; view?.dispose(); if (controller.current === view) controller.current = null; };
  }, [controller]);
  useEffect(() => { controller.current?.update(props.pages, props.edges, props.selected, props.groups); }, [controller, props.pages, props.edges, props.selected, props.groups]);
  return <div className="relative mt-2 h-80 overflow-hidden rounded-md border border-border">
    <div ref={container} className="h-full w-full" />
    {loading && <p role="status" className="pointer-events-none absolute inset-0 grid place-items-center text-sm text-text-secondary">3D 그래프를 준비하고 있습니다…</p>}
  </div>;
}
