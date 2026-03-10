import { Badge } from "@/components/ui/badge";

const variantMap: Record<
  string,
  "default" | "secondary" | "outline" | "destructive"
> = {
  approved: "default",
  voting: "default",
  active: "default",
  true: "default",
  rejected: "destructive",
  false: "destructive",
  finished: "secondary",
  deleted: "secondary",
  proposal: "outline",
  vote: "outline",
  delegation: "outline",
  lockup: "outline",
  reward: "outline",
  admin: "outline",
  config: "outline",
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
      variant={variantMap[value] ?? "secondary"}
      className="px-2.5 py-1 text-[0.7rem] font-semibold uppercase tracking-[0.18em]"
    >
      {label ?? value}
    </Badge>
  );
}
