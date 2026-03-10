import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

const toneMap: Record<string, string> = {
  created: "bg-amber-100 text-amber-900",
  approved: "bg-cyan-100 text-cyan-900",
  voting: "bg-emerald-100 text-emerald-900",
  finished: "bg-slate-200 text-slate-900",
  rejected: "bg-rose-100 text-rose-900",
  active: "bg-emerald-100 text-emerald-900",
  unlocking: "bg-amber-100 text-amber-900",
  deleted: "bg-slate-200 text-slate-900",
  true: "bg-emerald-100 text-emerald-900",
  false: "bg-rose-100 text-rose-900",
  proposal: "bg-cyan-100 text-cyan-900",
  vote: "bg-indigo-100 text-indigo-900",
  delegation: "bg-fuchsia-100 text-fuchsia-900",
  lockup: "bg-orange-100 text-orange-900",
  reward: "bg-lime-100 text-lime-900",
  admin: "bg-slate-200 text-slate-900",
  config: "bg-teal-100 text-teal-900",
};

export function StatusBadge({
  value,
  label,
}: {
  value: string;
  label?: string;
}) {
  return (
    <Badge
      variant="secondary"
      className={cn(
        "border-transparent px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em]",
        toneMap[value] ?? "bg-[color:var(--color-accent)] text-foreground",
      )}
    >
      {label ?? value}
    </Badge>
  );
}
