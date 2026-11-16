import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { AppearanceTab } from "@/components/settings/appearance-tab";
import { usePrefs } from "@/lib/store";
import { ThemeSync } from "@/lib/theme";
import { PREFS_STORAGE_KEY } from "@/lib/theme-boot";

beforeAll(() => {
  // jsdom has no matchMedia; report a light OS.
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
});

afterEach(() => {
  cleanup();
  act(() => usePrefs.getState().setTheme("system"));
});

function renderTab() {
  return render(
    <>
      <ThemeSync />
      <AppearanceTab />
    </>,
  );
}

describe("Appearance theme toggle", () => {
  it("defaults to following the system", () => {
    renderTab();
    expect(screen.getByRole("button", { name: "System" }).getAttribute("aria-pressed")).toBe("true");
    expect(document.documentElement.dataset.theme).toBeUndefined();
    expect(screen.getByText("Following your system: light right now.")).toBeTruthy();
  });

  it("sets data-theme on <html> and remembers the choice", () => {
    renderTab();
    fireEvent.click(screen.getByRole("button", { name: "Dark" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByRole("button", { name: "Dark" }).getAttribute("aria-pressed")).toBe("true");
    expect(JSON.parse(localStorage.getItem(PREFS_STORAGE_KEY) ?? "{}").state.theme).toBe("dark");

    fireEvent.click(screen.getByRole("button", { name: "System" }));
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });
});
