"use client";
import { Dialog as D } from "radix-ui";
import { X } from "lucide-react";
import { cn } from "@/lib/cn";

/** Modal dialog: focus is trapped and restored by Radix (design.md § Accessibility). */
export function Dialog({ open, onOpenChange, title, description, children, footer, wide }: { open: boolean; onOpenChange: (o: boolean) => void; title: string; description?: string; children: React.ReactNode; footer?: React.ReactNode; wide?: boolean }) {
  return (
    <D.Root open={open} onOpenChange={onOpenChange}>
      <D.Portal>
        <D.Overlay className="fixed inset-0 z-[400] bg-[var(--scrim)]" />
        <D.Content
          className={cn(
            "fixed top-1/2 left-1/2 z-[401] flex max-h-[min(88vh,840px)] w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-[var(--radius-card)] border border-rule bg-white shadow-lift",
            wide ? "max-w-3xl" : "max-w-lg",
          )}
        >
          <div className="flex items-start justify-between gap-4 border-b border-rule p-4">
            <div className="flex flex-col gap-1">
              <D.Title asChild>
                <h2>{title}</h2>
              </D.Title>
              {description ? <D.Description className="text-ink-2">{description}</D.Description> : <D.Description className="visually-hidden">{title}</D.Description>}
            </div>
            <D.Close className="btn btn-ghost btn-icon" aria-label="Close">
              <X aria-hidden className="size-4" />
            </D.Close>
          </div>
          <div className="min-h-0 overflow-y-auto p-4">{children}</div>
          {footer ? <div className="flex flex-wrap justify-end gap-2 border-t border-rule p-4">{footer}</div> : null}
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}

export function ConfirmDialog({ open, onOpenChange, title, body, confirmLabel, onConfirm, loading }: { open: boolean; onOpenChange: (o: boolean) => void; title: string; body: React.ReactNode; confirmLabel: string; onConfirm: () => void; loading?: boolean }) {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={title}
      footer={
        <>
          <button className="btn" onClick={() => onOpenChange(false)}>
            Keep it
          </button>
          <button className="btn btn-danger-solid" onClick={onConfirm} disabled={loading} aria-busy={loading || undefined}>
            {confirmLabel}
          </button>
        </>
      }
    >
      <div className="text-ink-2">{body}</div>
    </Dialog>
  );
}
