import { Wordmark } from "@/components/shell/wordmark";

export default function AuthLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="grid min-h-dvh grid-cols-[minmax(0,1fr)] min-[60rem]:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
      <div className="flex flex-col justify-between gap-10 bg-ink p-[var(--gutter)] text-paper max-[60rem]:hidden">
        <Wordmark inverse />
        <div className="flex max-w-md flex-col gap-4">
          <p className="font-display text-[2.4rem] leading-[1.02] font-bold tracking-tight">Build. Verify. Deliver.</p>
          <p className="text-paper-3">AI reasons. Rust executes. Evidence proves. Humans govern.</p>
        </div>
        <p className="text-xs text-paper-3">Self-hosted. Apache 2.0.</p>
      </div>
      <div className="grid place-items-center p-[var(--gutter)]">
        <div className="w-full max-w-sm">{children}</div>
      </div>
    </div>
  );
}
