import { afterEach, describe, expect, it, vi } from "vitest";
import { bytes, duration, percent, relative, shortId } from "@/lib/format";

describe("format", () => {
  afterEach(() => vi.useRealTimers());

  it("formats durations at each scale", () => {
    expect(duration(null)).toBe("–");
    expect(duration(420)).toBe("420 ms");
    expect(duration(2345)).toBe("2.35 s");
    expect(duration(12_300)).toBe("12.3 s");
    expect(duration(125_000)).toBe("2 min 5 s");
  });

  it("never shows a percentage for missing data", () => {
    expect(percent(null)).toBe("–");
    expect(percent(undefined)).toBe("–");
    expect(percent(0.875)).toBe("87.5%");
    expect(percent(1, 0)).toBe("100%");
  });

  it("formats relative times", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-13T12:00:00Z"));
    expect(relative(null)).toBe("never");
    expect(relative("2026-11-13T11:59:50Z")).toBe("just now");
    expect(relative("2026-11-13T11:50:00Z")).toBe("10 min ago");
    expect(relative("2026-11-13T09:00:00Z")).toBe("3 h ago");
    expect(relative("2026-11-10T12:00:00Z")).toBe("3 d ago");
  });

  it("formats sizes and short ids", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(2048)).toBe("2.0 KB");
    expect(bytes(5 * 1024 * 1024)).toBe("5.0 MB");
    expect(shortId("0193a2b4-1111-2222-3333-abcdef123456")).toBe("123456");
  });
});
