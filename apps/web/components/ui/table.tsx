import { cn } from "@/lib/cn";

/** Dense collection table (design.md § Tables). Rows restack under 40rem via data-label. */
export function Table({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={cn("oryn-table-wrap w-full overflow-x-auto", className)}>
      <table className="oryn-table w-full border-collapse text-sm">{children}</table>
    </div>
  );
}

export function Th({ children, className, numeric }: { children?: React.ReactNode; className?: string; numeric?: boolean }) {
  return <th className={cn("h-10 bg-paper-2 px-3 text-left text-xs font-medium whitespace-nowrap text-ink-2 first:rounded-l-[var(--radius-input)] last:rounded-r-[var(--radius-input)]", numeric && "text-right", className)}>{children}</th>;
}

export function Td({ children, className, numeric, label, mono }: { children?: React.ReactNode; className?: string; numeric?: boolean; label?: string; mono?: boolean }) {
  return (
    <td data-label={label} className={cn("h-11 border-b border-rule px-3 align-middle", numeric && "text-right tabular-nums", mono && "mono", className)}>
      {children}
    </td>
  );
}

export function Tr({ children, className, onClick }: { children: React.ReactNode; className?: string; onClick?: () => void }) {
  return (
    <tr onClick={onClick} className={cn("transition-colors duration-[var(--dur-micro)] hover:bg-paper-3", onClick && "cursor-pointer", className)}>
      {children}
    </tr>
  );
}
