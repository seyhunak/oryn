"use client";
import Editor, { type BeforeMount, type Monaco, type OnMount } from "@monaco-editor/react";
import { useEffect, useRef } from "react";
import { Skeleton } from "@/components/ui/states";

type CodeEditor = Parameters<OnMount>[0];
export type EditorDiagnostic = { line: number; message: string; severity: "error" | "warning" };
export type GherkinEditorHandle = { insert: (text: string) => void; focus: () => void };

/**
 * Read a design token from the document and return it as `#rrggbb` (Monaco only accepts hex).
 * Tokens are authored in oklch; a 1px canvas converts whatever the browser resolves into sRGB.
 * Returns undefined when the token is missing, in which case Monaco keeps its own default.
 */
function tokenHex(name: string): string | undefined {
  if (typeof document === "undefined") return undefined;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  if (!value) return undefined;
  const c = document.createElement("canvas");
  c.width = c.height = 1;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (!ctx) return undefined;
  ctx.fillStyle = value;
  ctx.fillRect(0, 0, 1, 1);
  const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
  return `#${[r, g, b].map((n) => n.toString(16).padStart(2, "0")).join("")}`;
}

function strip(hex: string | undefined) {
  return hex ? hex.slice(1) : undefined;
}

let registered = false;

/** Register the Gherkin tokenizer and the `oryn-paper` theme (colours read from tokens.css at mount). */
function setup(monaco: Monaco) {
  if (!registered) {
    registered = true;
    monaco.languages.register({ id: "gherkin" });
    monaco.languages.setMonarchTokensProvider("gherkin", {
      tokenizer: {
        root: [
          [/^\s*#.*$/, "comment"],
          [/^\s*(Feature|Background|Scenario Outline|Scenario Template|Scenario|Example|Examples|Scenarios|Rule):/, "keyword"],
          [/^\s*(Given|When|Then|And|But|\*)(?=\s)/, "keyword.step"],
          [/@[^\s@]+/, "tag"],
          [/"""/, { token: "string", next: "@docstring" }],
          [/"/, { token: "string", next: "@string" }],
          [/<[^>\s]+>/, "variable"],
          [/\{\{[^}]+\}\}/, "variable"],
          [/\|/, "delimiter"],
          [/\d+(\.\d+)?/, "number"],
        ],
        string: [
          [/<[^>\s]+>/, "variable"],
          [/\{\{[^}]+\}\}/, "variable"],
          [/[^"<{]+/, "string"],
          [/[<{]/, "string"],
          [/"/, { token: "string", next: "@pop" }],
        ],
        docstring: [
          [/"""/, { token: "string", next: "@pop" }],
          [/.*/, "string"],
        ],
      },
    });
  }
  const t = {
    paper: tokenHex("--color-paper"),
    paper2: tokenHex("--color-paper-2"),
    paper3: tokenHex("--color-paper-3"),
    ink: tokenHex("--color-ink"),
    ink2: tokenHex("--color-ink-2"),
    muted: tokenHex("--color-muted"),
    rule: tokenHex("--color-rule"),
    accent: tokenHex("--color-accent"),
    deep: tokenHex("--color-accent-deep"),
    soft: tokenHex("--color-accent-soft"),
    error: tokenHex("--color-error"),
    white: tokenHex("--white"),
  };
  const colors: Record<string, string | undefined> = {
    "editor.background": t.white,
    "editor.foreground": t.ink,
    "editorGutter.background": t.white,
    "editorLineNumber.foreground": t.muted,
    "editorLineNumber.activeForeground": t.ink,
    "editor.lineHighlightBackground": t.paper2,
    "editor.lineHighlightBorder": t.paper2,
    "editor.selectionBackground": t.soft,
    "editor.inactiveSelectionBackground": t.paper3,
    "editorCursor.foreground": t.ink,
    "editorIndentGuide.background1": t.rule,
    "editorIndentGuide.activeBackground1": t.muted,
    "editorWhitespace.foreground": t.rule,
    "editorError.foreground": t.error,
    "editorWarning.foreground": t.deep,
    "editorWidget.background": t.white,
    "editorWidget.border": t.rule,
    "editorHoverWidget.background": t.white,
    "editorHoverWidget.border": t.rule,
    "scrollbarSlider.background": t.rule,
    "focusBorder": t.accent,
  };
  monaco.editor.defineTheme("oryn-paper", {
    base: "vs",
    inherit: true,
    rules: [
      { token: "", foreground: strip(t.ink) },
      { token: "keyword", foreground: strip(t.deep), fontStyle: "bold" },
      { token: "keyword.step", foreground: strip(t.accent), fontStyle: "bold" },
      { token: "tag", foreground: strip(t.muted) },
      { token: "comment", foreground: strip(t.muted), fontStyle: "italic" },
      { token: "string", foreground: strip(t.ink2) },
      { token: "variable", foreground: strip(t.deep), fontStyle: "underline" },
      { token: "delimiter", foreground: strip(t.muted) },
      { token: "number", foreground: strip(t.ink2) },
    ],
    colors: Object.fromEntries(Object.entries(colors).filter(([, v]) => !!v)) as Record<string, string>,
  });
}

/** Gherkin editor (Monaco, `oryn-paper` theme derived from the design tokens). */
export function GherkinEditor({
  value,
  onChange,
  readOnly,
  diagnostics,
  height = 520,
  label,
  onReady,
  revealLine,
}: {
  value: string;
  onChange?: (v: string) => void;
  readOnly?: boolean;
  diagnostics?: EditorDiagnostic[];
  height?: number | string;
  label: string;
  onReady?: (h: GherkinEditorHandle) => void;
  /** Scroll to and place the cursor on this 1-based line (e.g. the selected scenario). */
  revealLine?: number | null;
}) {
  const editorRef = useRef<CodeEditor | null>(null);
  const monacoRef = useRef<Monaco | null>(null);

  const beforeMount: BeforeMount = (monaco) => setup(monaco);
  const onMount: OnMount = (ed, monaco) => {
    editorRef.current = ed;
    monacoRef.current = monaco;
    applyMarkers();
    reveal();
    onReady?.({
      focus: () => ed.focus(),
      insert: (text) => {
        const sel = ed.getSelection();
        const model = ed.getModel();
        if (!sel || !model) return;
        const lineText = model.getLineContent(sel.startLineNumber);
        const atBlankLine = lineText.trim() === "" && sel.isEmpty();
        const insertText = atBlankLine && sel.startColumn === 1 ? `    ${text}` : text;
        ed.executeEdits("step-library", [{ range: sel, text: insertText, forceMoveMarkers: true }]);
        ed.focus();
      },
    });
  };

  function applyMarkers() {
    const ed = editorRef.current;
    const monaco = monacoRef.current;
    const model = ed?.getModel();
    if (!monaco || !model) return;
    const lines = model.getLineCount();
    monaco.editor.setModelMarkers(
      model,
      "oryn",
      (diagnostics ?? []).map((d) => {
        const line = Math.min(Math.max(d.line, 1), lines);
        return {
          startLineNumber: line,
          endLineNumber: line,
          startColumn: model.getLineFirstNonWhitespaceColumn(line) || 1,
          endColumn: model.getLineMaxColumn(line),
          message: d.message,
          severity: d.severity === "error" ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
        };
      }),
    );
  }

  function reveal() {
    const ed = editorRef.current;
    if (!ed || !revealLine) return;
    ed.revealLineInCenter(revealLine);
    ed.setPosition({ lineNumber: revealLine, column: 1 });
  }

  useEffect(applyMarkers, [diagnostics]);
  useEffect(reveal, [revealLine]);

  return (
    <div className="overflow-hidden rounded-[var(--radius-input)] border border-rule bg-white">
      <Editor
        height={height}
        language="gherkin"
        theme="oryn-paper"
        value={value}
        onChange={(v) => onChange?.(v ?? "")}
        beforeMount={beforeMount}
        onMount={onMount}
        loading={<Skeleton className="m-3 h-[calc(100%-24px)] w-[calc(100%-24px)]" />}
        options={{
          readOnly,
          ariaLabel: label,
          fontFamily: typeof document === "undefined" ? undefined : getComputedStyle(document.documentElement).getPropertyValue("--font-outlier").trim() || undefined,
          fontSize: 13,
          lineHeight: 20,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          renderLineHighlight: readOnly ? "none" : "line",
          tabSize: 2,
          insertSpaces: true,
          wordWrap: "on",
          padding: { top: 12, bottom: 12 },
          automaticLayout: true,
          fixedOverflowWidgets: true,
          domReadOnly: readOnly,
          lineNumbersMinChars: 3,
          overviewRulerLanes: 0,
          scrollbar: { alwaysConsumeMouseWheel: false },
        }}
      />
    </div>
  );
}
