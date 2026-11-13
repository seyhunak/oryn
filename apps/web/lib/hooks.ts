"use client";
import { useQuery } from "@tanstack/react-query";
import { useSearchParams } from "next/navigation";
import { useEffect } from "react";
import { api, unwrap, type Schemas } from "./api/client";
import { usePrefs } from "./store";

export const keys = {
  me: ["me"] as const,
  projects: (ws: string) => ["ws", ws, "projects"] as const,
  ws: (ws: string, ...rest: unknown[]) => ["ws", ws, ...rest] as const,
  project: (id: string, ...rest: unknown[]) => ["project", id, ...rest] as const,
};

export function useMe() {
  return useQuery({ queryKey: keys.me, queryFn: () => unwrap(api.GET("/api/v1/auth/me")), retry: false, staleTime: 60_000 });
}

/** The current workspace: the persisted choice when it is still one of the user's, else the first. */
export function useWorkspace(): { ws: Schemas["WorkspaceWithRole"] | null; role: Schemas["Role"] | null; me: Schemas["Me"] | undefined } {
  const me = useMe().data;
  const { workspaceId, setWorkspace } = usePrefs();
  const ws = me?.workspaces.find((w) => w.id === workspaceId) ?? me?.workspaces[0] ?? null;
  useEffect(() => {
    if (ws && ws.id !== workspaceId) setWorkspace(ws.id);
  }, [ws, workspaceId, setWorkspace]);
  return { ws, role: (ws?.role as Schemas["Role"]) ?? null, me };
}

/** Workspace id, for components rendered inside the authenticated shell (always resolved there). */
export function useWsId(): string {
  return useWorkspace().ws?.id ?? "";
}

const RANK: Record<string, number> = { viewer: 0, engineer: 1, admin: 2, owner: 3 };
/** UI hint only: the API enforces every permission server-side. */
export function useCan(min: "viewer" | "engineer" | "admin" | "owner") {
  const { role } = useWorkspace();
  return role != null && RANK[role] >= RANK[min];
}

export function useProjects() {
  const ws = useWsId();
  return useQuery({
    queryKey: keys.projects(ws),
    enabled: !!ws,
    queryFn: () => unwrap(api.GET("/api/v1/workspaces/{ws}/projects", { params: { path: { ws } } })),
  });
}

/**
 * The project a project-scoped screen works on: `?project=` wins, then the remembered
 * choice, then the first project. Returns null while loading or when there are none.
 */
export function useCurrentProject() {
  const sp = useSearchParams();
  const fromUrl = sp.get("project");
  const { projectId, setProject } = usePrefs();
  const projects = useProjects();
  const list = projects.data ?? [];
  const project = list.find((p) => p.id === fromUrl) ?? list.find((p) => p.id === projectId) ?? list[0] ?? null;
  useEffect(() => {
    if (project && project.id !== projectId) setProject(project.id);
  }, [project, projectId, setProject]);
  return { project, projects: list, isPending: projects.isPending, error: projects.error, refetch: projects.refetch };
}
