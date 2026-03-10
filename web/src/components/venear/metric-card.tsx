import Link from "next/link";
import { ArrowUpRight } from "lucide-react";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { buttonVariants } from "@/components/ui/button-styles";
import { cn } from "@/lib/utils";

export function MetricCard({
  label,
  value,
  supporting,
  href,
}: {
  label: string;
  value: string;
  supporting: string;
  href?: string;
}) {
  return (
    <Card className="border-white/60 bg-white/80 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
      <CardHeader className="gap-2">
        <p className="text-xs font-semibold uppercase tracking-[0.24em] text-muted-foreground">
          {label}
        </p>
        <CardTitle className="text-3xl tracking-[-0.04em]">{value}</CardTitle>
      </CardHeader>
      <CardContent className="flex items-end justify-between gap-4">
        <p className="text-sm leading-6 text-muted-foreground">{supporting}</p>
        {href ? (
          <Link
            href={href}
            className={cn(
              buttonVariants({ variant: "outline", size: "sm" }),
              "shrink-0 border-foreground/10 bg-white/70",
            )}
          >
            Open
            <ArrowUpRight className="size-4" />
          </Link>
        ) : null}
      </CardContent>
    </Card>
  );
}
