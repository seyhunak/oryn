import { forwardRef, useId } from "react";
import { cn } from "@/lib/cn";

type FieldShellProps = { label: string; hint?: string; error?: string | null; children: (id: string, describedBy?: string) => React.ReactNode; className?: string };

/** Label + control + reserved help/error line (design.md § Inputs). */
export function Field({ label, hint, error, children, className }: FieldShellProps) {
  const id = useId();
  const helpId = `${id}-help`;
  return (
    <div className={cn("flex min-w-0 flex-col gap-1", className)}>
      <label htmlFor={id} className="text-sm font-medium text-ink-2">
        {label}
      </label>
      {children(id, hint || error ? helpId : undefined)}
      <p id={helpId} className={cn("min-h-[1lh] text-xs", error ? "text-error" : "text-muted")}>
        {error ?? hint ?? ""}
      </p>
    </div>
  );
}

export const Input = forwardRef<HTMLInputElement, React.InputHTMLAttributes<HTMLInputElement>>(function Input({ className, ...p }, ref) {
  return <input ref={ref} className={cn("field", className)} {...p} />;
});

export const Textarea = forwardRef<HTMLTextAreaElement, React.TextareaHTMLAttributes<HTMLTextAreaElement>>(function Textarea({ className, ...p }, ref) {
  return <textarea ref={ref} className={cn("field", className)} {...p} />;
});

export const Select = forwardRef<HTMLSelectElement, React.SelectHTMLAttributes<HTMLSelectElement>>(function Select({ className, ...p }, ref) {
  return <select ref={ref} className={cn("field pr-8", className)} {...p} />;
});

export function Checkbox({ label, ...p }: React.InputHTMLAttributes<HTMLInputElement> & { label: string }) {
  return (
    <label className="inline-flex min-h-11 cursor-pointer items-center gap-2 text-sm">
      <input type="checkbox" className="size-4 accent-[var(--color-accent)]" {...p} />
      {label}
    </label>
  );
}
