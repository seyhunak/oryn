"use client";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { ThemePref } from "@/lib/theme";
import { PREFS_STORAGE_KEY } from "@/lib/theme-boot";

/** UI preferences that survive reloads. Server data lives in TanStack Query, not here. */
type Prefs = {
  workspaceId: string | null;
  projectId: string | null;
  theme: ThemePref;
  setWorkspace: (id: string | null) => void;
  setProject: (id: string | null) => void;
  setTheme: (theme: ThemePref) => void;
};

export const usePrefs = create<Prefs>()(
  persist(
    (set) => ({
      workspaceId: null,
      projectId: null,
      theme: "system",
      setWorkspace: (workspaceId) => set({ workspaceId, projectId: null }),
      setProject: (projectId) => set({ projectId }),
      setTheme: (theme) => set({ theme }),
    }),
    // The pre-paint theme script (lib/theme-boot.ts) reads this key too.
    { name: PREFS_STORAGE_KEY },
  ),
);
