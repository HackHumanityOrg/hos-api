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
    <Card>
      <CardHeader className="gap-2">
        <p className="text-xs font-semibold uppercase tracking-[0.24em] text-muted-foreground">
          {label}
        </p>
        <CardTitle className="text-3xl tracking-[-0.04em]">{value}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <p className="min-w-0 text-sm leading-6 text-muted-foreground break-words">
          {supporting}
        </p>
        {href ? (
          <Link
            href={href}
            className={cn(buttonVariants({ variant: "outline", size: "sm" }), "shrink-0")}
          >
            Open
            <ArrowUpRight className="size-4" />
          </Link>
        ) : null}
      </CardContent>
    </Card>
  );
}
