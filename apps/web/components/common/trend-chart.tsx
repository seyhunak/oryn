"use client";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Schemas } from "@/lib/api/client";

/** Final-attempt job outcomes per day. Drawn only from measured data (design.md § Data visualisation). */
export function TrendChart({ data }: { data: Schemas["TrendPoint"][] }) {
  const fmt = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", timeZone: "UTC" });
  const rows = data.map((d) => ({ day: fmt.format(new Date(`${d.day.slice(0, 10)}T00:00:00Z`)), passed: d.passed, failed: d.failed + d.errored }));
  return (
    <figure className="m-0 flex flex-col gap-2">
      <div className="h-56 w-full" role="img" aria-label={`Job outcomes per day over ${data.length} days`}>
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: -16 }} barCategoryGap="28%">
            <CartesianGrid vertical={false} stroke="var(--color-rule)" />
            <XAxis dataKey="day" tickLine={false} axisLine={false} tick={{ fill: "var(--color-muted)", fontSize: 11 }} interval={Math.max(0, Math.ceil(rows.length / 7) - 1)} />
            <YAxis allowDecimals={false} tickLine={false} axisLine={false} tick={{ fill: "var(--color-muted)", fontSize: 11 }} />
            <Tooltip
              cursor={{ fill: "var(--color-paper-3)" }}
              contentStyle={{ background: "var(--white)", border: "1px solid var(--color-rule)", borderRadius: 10, fontSize: 12 }}
            />
            <Bar dataKey="passed" name="Passed" stackId="a" fill="var(--color-accent)" />
            <Bar dataKey="failed" name="Failed or error" stackId="a" fill="var(--color-error)" radius={[3, 3, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </div>
      <figcaption className="flex gap-4 text-xs text-ink-2">
        <span className="inline-flex items-center gap-1.5">
          <span aria-hidden className="size-2.5 rounded-sm bg-accent" /> Passed
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span aria-hidden className="size-2.5 rounded-sm bg-error" /> Failed or error
        </span>
      </figcaption>
    </figure>
  );
}
