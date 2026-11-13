import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AiTag, StatusPill } from "@/components/ui/status";

describe("StatusPill", () => {
  it("always pairs a glyph with a word", () => {
    const { container } = render(<StatusPill status="failed" />);
    expect(screen.getByText("failed")).toBeTruthy();
    expect(container.querySelector("svg")).toBeTruthy();
  });

  it("humanises multi-word statuses", () => {
    render(<StatusPill status="timed_out" />);
    expect(screen.getByText("timed out")).toBeTruthy();
  });

  it("falls back to the waiting tone for unknown statuses", () => {
    render(<StatusPill status="something_new" />);
    expect(screen.getByText("something new").className).toContain("border-rule");
  });
});

describe("AiTag", () => {
  it("labels AI content and shows confidence as a figure", () => {
    render(<AiTag confidence={0.82} />);
    expect(screen.getByText(/AI interpretation/)).toBeTruthy();
    expect(screen.getByText("82%")).toBeTruthy();
  });
});
