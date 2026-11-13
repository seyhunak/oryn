"use client";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useCurrentProject } from "@/lib/hooks";
import { usePrefs } from "@/lib/store";

/** Selects the project a project-scoped screen works on; the choice goes into `?project=`. */
export function ProjectPicker({ allowAll, label = "Project" }: { allowAll?: boolean; label?: string }) {
  const { project, projects } = useCurrentProject();
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const setProject = usePrefs((s) => s.setProject);
  const all = allowAll && sp.get("project") === "all";
  if (projects.length === 0) return null;
  return (
    <label className="flex items-center gap-2 text-sm text-ink-2">
      {label}
      <select
        className="field h-9 w-auto max-w-[16rem]"
        value={all ? "all" : (project?.id ?? "")}
        onChange={(e) => {
          const q = new URLSearchParams(sp.toString());
          q.set("project", e.target.value);
          q.delete("page");
          if (e.target.value !== "all") setProject(e.target.value);
          router.push(`${path}?${q.toString()}`);
        }}
      >
        {allowAll ? <option value="all">All projects</option> : null}
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
    </label>
  );
}
