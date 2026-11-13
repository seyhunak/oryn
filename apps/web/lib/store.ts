"use client";
import { create } from "zustand";
import { persist } from "zustand/middleware";

/** UI preferences that survive reloads. Server data lives in TanStack Query, not here. */
type Prefs = {
  workspaceId: string | null;
  projectId: string | null;
  setWorkspace: (id: string | null) => void;
  setProject: (id: string | null) => void;
};

export const usePrefs = create<Prefs>()(
  persist(
    (set) => ({
      workspaceId: null,
      projectId: null,
      setWorkspace: (workspaceId) => set({ workspaceId, projectId: null }),
      setProject: (projectId) => set({ projectId }),
    }),
    { name: "oryn.prefs" },
  ),
);
