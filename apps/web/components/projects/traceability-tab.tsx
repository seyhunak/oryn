"use client";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { api, unwrap, type Schemas } from "@/lib/api/client";
import { keys } from "@/lib/hooks";
import { percent } from "@/lib/format";
import { Section, Stat } from "@/components/ui/page";
import { Empty, ErrorState, QueryState, Skeleton } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { buttonClass } from "@/components/ui/button";
import type { ProjectDetail } from "./common";

type Row = Schemas["RequirementCoverageRow"];

/** Coverage state of one requirement, derived only from the API's counts. */
function rowState(r: Row): { status: string; label: string } {
  if (r.executable_scenarios === 0) return { status: "uncovered", label: "uncovered" };
  if (r.failing > 0) return { status: "failed", label: "failing" };
  if (r.never_run > 0) return { status: "not_run", label: "not all run" };
  return { status: "verified", label: "verified" };
}

const FILTERS = [
  { id: "", label: "All requirements" },
  { id: "uncovered", label: "Uncovered" },
  { id: "failed", label: "Failing" },
  { id: "not_run", label: "Not all run" },
  { id: "verified", label: "Verified" },
];

export function TraceabilityTab({ project }: { project: ProjectDetail }) {
  const id = project.id;
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const filter = sp.get("coverage") ?? "";
  const trace = useQuery({
    queryKey: keys.project(id, "traceability"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/traceability", { params: { path: { id } } })),
  });
  const scenarios = useQuery({
    queryKey: keys.project(id, "scenarios"),
    queryFn: () => unwrap(api.GET("/api/v1/projects/{id}/scenarios", { params: { path: { id } } })),
  });
  const setFilter = (v: string) => {
    const q = new URLSearchParams(sp.toString());
    if (v) q.set("coverage", v);
    else q.delete("coverage");
    router.replace(`${path}?${q.toString()}`, { scroll: false });
  };
  const planner = (
    <Link href={`/planner?project=${id}`} className={buttonClass("primary")}>
      Plan tests for gaps
    </Link>
  );

  return (
    <div className="flex flex-col gap-10">
      <Section
        title="Coverage"
        description="Covered means linked to at least one approved, executable scenario. Verified means every linked scenario's latest final run passed."
        actions={planner}
      >
        <QueryState
          query={trace}
          skeleton={
            <div className="grid grid-cols-2 gap-6 md:grid-cols-5">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-14" />
              ))}
            </div>
          }
        >
          {(t) => {
            const c = t.coverage;
            return (
              <div className="grid grid-cols-2 gap-x-6 gap-y-4 border-y border-rule py-4 md:grid-cols-5">
                <Stat label="Requirements" value={c.requirements} />
                <Stat label="Covered" value={c.covered} hint={c.coverage_rate == null ? "no requirements" : `${percent(c.coverage_rate)} of requirements`} />
                <Stat label="Uncovered" value={c.requirements - c.covered} hint="no executable scenario linked" />
                <Stat label="Verified" value={c.verified} hint={c.verified_rate == null ? "no requirements" : `${percent(c.verified_rate)} of requirements`} />
                <Stat label="Failing" value={c.failing} hint="a linked scenario's latest run failed" />
              </div>
            );
          }}
        </QueryState>
      </Section>

      <Section
        title="Requirement to scenario"
        description="Each requirement with the scenarios linked to it and their latest final status."
        actions={
          <label className="flex items-center gap-2 text-sm text-ink-2">
            Show
            <select className="field h-9 w-auto" value={filter} onChange={(e) => setFilter(e.target.value)}>
              {FILTERS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </select>
          </label>
        }
      >
        {scenarios.error ? <ErrorState error={scenarios.error} retry={() => scenarios.refetch()} /> : null}
        <QueryState
          query={trace}
          isEmpty={(t) => t.coverage.rows.length === 0}
          empty={
            <Empty
              title="No requirements to trace."
              action={
                <Link href={`/projects/${id}?tab=requirements`} className={buttonClass("secondary")}>
                  Add requirements
                </Link>
              }
            >
              Add or import requirements, then link scenarios to them.
            </Empty>
          }
        >
          {(t) => {
            const byReq = new Map<string, Schemas["TraceLink"][]>();
            for (const l of t.links) byReq.set(l.requirement_id, [...(byReq.get(l.requirement_id) ?? []), l]);
            const status = new Map((scenarios.data ?? []).map((s) => [s.id, s]));
            const rows = t.coverage.rows.filter((r) => !filter || rowState(r).status === filter);
            if (rows.length === 0) return <Empty title="No requirements match this filter." action={<button className="btn" onClick={() => setFilter("")}>Show all</button>} />;
            return (
              <Table>
                <thead>
                  <tr>
                    <Th>Requirement</Th>
                    <Th>Coverage</Th>
                    <Th>Linked scenarios</Th>
                    <Th numeric>Passing</Th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => {
                    const st = rowState(r);
                    const links = byReq.get(r.requirement_id) ?? [];
                    const uncovered = st.status === "uncovered";
                    return (
                      <Tr key={r.requirement_id} className={uncovered ? "bg-paper-2" : undefined}>
                        <Td label="Requirement" className="align-top">
                          <div className="flex items-baseline gap-2 pt-2">
                            <span className="mono text-xs font-medium whitespace-nowrap">{r.key}</span>
                            <span className="font-medium">{r.title}</span>
                          </div>
                          <Tag className="mt-1 mb-2">{r.priority}</Tag>
                        </Td>
                        <Td label="Coverage" className="align-top">
                          <div className="flex flex-col items-start gap-1 py-2">
                            <StatusPill status={st.status} label={st.label} />
                            {uncovered ? (
                              <Link href={`/planner?project=${id}&requirement=${r.requirement_id}`} className="text-xs">
                                Plan scenarios
                              </Link>
                            ) : null}
                          </div>
                        </Td>
                        <Td label="Linked scenarios" className="align-top">
                          {links.length === 0 ? (
                            <p className="py-2 text-muted">None linked</p>
                          ) : (
                            <ul className="flex flex-col py-1">
                              {links.map((l) => {
                                const s = status.get(l.scenario_id);
                                return (
                                  <li key={l.scenario_id} className="flex items-center justify-between gap-3 py-1">
                                    <span className="min-w-0 truncate" title={l.scenario_name}>
                                      {l.scenario_name}
                                    </span>
                                    {scenarios.isPending ? (
                                      <Skeleton className="h-5 w-16" />
                                    ) : s?.last_status ? (
                                      <StatusPill status={s.last_status} />
                                    ) : (
                                      <StatusPill status="not_run" />
                                    )}
                                  </li>
                                );
                              })}
                            </ul>
                          )}
                        </Td>
                        <Td label="Passing" numeric mono className="align-top">
                          <div className="py-2">
                            {r.passing}/{r.executable_scenarios}
                          </div>
                        </Td>
                      </Tr>
                    );
                  })}
                </tbody>
              </Table>
            );
          }}
        </QueryState>
      </Section>
    </div>
  );
}
