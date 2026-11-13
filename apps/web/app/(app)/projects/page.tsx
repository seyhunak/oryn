"use client";
import Link from "next/link";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { Plus } from "lucide-react";
import { useCan, useProjects, useWorkspace } from "@/lib/hooks";
import { relative } from "@/lib/format";
import { PageHeader } from "@/components/ui/page";
import { Empty, QueryState } from "@/components/ui/states";
import { StatusPill, Tag } from "@/components/ui/status";
import { Table, Td, Th, Tr } from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { ProjectChip } from "@/components/projects/common";
import { NewProjectDialog } from "@/components/projects/new-project-dialog";

export default function ProjectsPage() {
  const { ws } = useWorkspace();
  const projects = useProjects();
  const canCreate = useCan("engineer");
  const sp = useSearchParams();
  const router = useRouter();
  const path = usePathname();
  const open = sp.get("new") === "1" && canCreate;
  const setOpen = (o: boolean) => {
    const q = new URLSearchParams(sp.toString());
    if (o) q.set("new", "1");
    else q.delete("new");
    const s = q.toString();
    router.replace(s ? `${path}?${s}` : path, { scroll: false });
  };

  return (
    <>
      <PageHeader
        title="Projects"
        description={`Applications under test in ${ws?.name ?? "this workspace"}. Each holds its repository, requirements, environments and suites.`}
        actions={
          canCreate ? (
            <Button variant="primary" size="lg" onClick={() => setOpen(true)}>
              <Plus aria-hidden className="size-4" /> New project
            </Button>
          ) : null
        }
      />
      <QueryState
        query={projects}
        empty={
          <Empty
            title="No projects yet."
            action={
              canCreate ? (
                <Button variant="primary" onClick={() => setOpen(true)}>
                  Create a project
                </Button>
              ) : undefined
            }
          >
            A project holds the application under test: its repository, requirements, environments and suites.
            {canCreate ? null : " Ask a workspace engineer or admin to create one."}
          </Empty>
        }
      >
        {(list) => (
          <Table>
            <thead>
              <tr>
                <Th>Project</Th>
                <Th numeric>Suites</Th>
                <Th numeric>Scenarios</Th>
                <Th>Repository</Th>
                <Th>Last execution</Th>
                <Th>Visibility</Th>
              </tr>
            </thead>
            <tbody>
              {list.map((p) => (
                <Tr key={p.id}>
                  <Td label="Project">
                    <div className="flex items-center gap-3">
                      <ProjectChip name={p.name} color={p.color} />
                      <div className="min-w-0">
                        <Link href={`/projects/${p.id}`} className="font-medium no-underline hover:underline">
                          {p.name}
                        </Link>
                        {p.description ? <div className="max-w-[48ch] truncate text-xs text-muted">{p.description}</div> : null}
                      </div>
                    </div>
                  </Td>
                  <Td label="Suites" numeric mono>
                    {p.suite_count}
                  </Td>
                  <Td label="Scenarios" numeric mono>
                    {p.scenario_count}
                  </Td>
                  <Td label="Repository">{p.repository_status ? <StatusPill status={p.repository_status} /> : <span className="text-muted">None connected</span>}</Td>
                  <Td label="Last execution">
                    {p.last_execution_status ? (
                      <span className="inline-flex items-center gap-2">
                        <StatusPill status={p.last_execution_status} />
                        <span className="text-xs text-ink-2">{relative(p.last_execution_at)}</span>
                      </span>
                    ) : (
                      <span className="text-muted">Never run</span>
                    )}
                  </Td>
                  <Td label="Visibility">
                    <Tag>{p.visibility === "private" ? "Private" : "Workspace"}</Tag>
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        )}
      </QueryState>
      <NewProjectDialog open={open} onOpenChange={setOpen} />
    </>
  );
}
