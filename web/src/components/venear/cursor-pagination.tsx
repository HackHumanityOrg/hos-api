import Link from "next/link";

import { buttonVariants } from "@/components/ui/button-styles";
import { buildHref, readSearchParam, type SearchParamsRecord } from "@/lib/search";
import { cn } from "@/lib/utils";

export function CursorPagination({
  pathname,
  searchParams,
  nextCursor,
  hasMore,
}: {
  pathname: string;
  searchParams: SearchParamsRecord;
  nextCursor: string | null;
  hasMore: boolean;
}) {
  const currentCursor = readSearchParam(searchParams, "cursor");

  if (!currentCursor && !hasMore) {
    return null;
  }

  return (
    <div className="flex flex-col gap-3 rounded-2xl border border-white/60 bg-white/70 p-4 shadow-[0_14px_40px_rgba(22,36,55,0.05)] sm:flex-row sm:items-center sm:justify-between">
      <p className="text-sm text-muted-foreground">
        {hasMore
          ? "This page is a live cursor window into the API. Continue forward with the opaque cursor."
          : "You have reached the end of this cursor window."}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        {currentCursor ? (
          <Link
            href={buildHref(pathname, searchParams, { cursor: null })}
            className={cn(buttonVariants({ variant: "outline", size: "sm" }))}
          >
            First page
          </Link>
        ) : null}
        {hasMore && nextCursor ? (
          <Link
            href={buildHref(pathname, searchParams, { cursor: nextCursor })}
            className={cn(buttonVariants({ size: "sm" }))}
          >
            Next page
          </Link>
        ) : null}
      </div>
    </div>
  );
}
