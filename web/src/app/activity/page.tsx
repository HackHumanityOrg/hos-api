import { Search } from "lucide-react";

import { AccountLink } from "@/components/venear/account-link";
import { CursorPagination } from "@/components/venear/cursor-pagination";
import { EmptyState } from "@/components/venear/empty-state";
import { PageHeader } from "@/components/venear/page-header";
import { StatusBadge } from "@/components/venear/status-badge";
import { Button } from "@/components/ui/button";
import { buttonVariants } from "@/components/ui/button-styles";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { formatTimestamp, humanizeSnakeCase, prettyJson } from "@/lib/format";
import { readSearchParam, type SearchParamsRecord } from "@/lib/search";
import { listActivity } from "@/lib/venear-api";
import Link from "next/link";

type Props = {
  searchParams: Promise<SearchParamsRecord>;
};

const kinds = ["proposal", "vote", "delegation", "lockup", "reward", "admin", "config"];

export default async function ActivityPage({ searchParams }: Props) {
  const params = await searchParams;
  const kind = readSearchParam(params, "kind");
  const accountId = readSearchParam(params, "account_id");
  const proposalId = readSearchParam(params, "proposal_id");
  const cursor = readSearchParam(params, "cursor");

  const activity = await listActivity({
    limit: 24,
    cursor,
    kind,
    account_id: accountId,
    proposal_id: proposalId,
  });

  return (
    <div className="space-y-8">
      <PageHeader
        eyebrow="Activity"
        title="One feed across proposal, vote, delegation, lockup, reward, admin, and config events."
        description="The activity feed exposes the cross-domain union payload directly. Filter it by kind, account, or proposal to pivot between governance moments."
      />

      <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
        <CardHeader>
          <CardTitle>Filters</CardTitle>
          <CardDescription>
            Narrow the global union feed before following the cursor forward.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="grid gap-3 md:grid-cols-[180px_1fr_180px_auto]">
            <label className="space-y-2">
              <span className="text-sm font-medium text-foreground">Kind</span>
              <select
                name="kind"
                defaultValue={kind ?? ""}
                className="h-8 rounded-lg border border-input bg-white/90 px-2.5 text-sm outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              >
                <option value="">All kinds</option>
                {kinds.map((item) => (
                  <option key={item} value={item}>
                    {humanizeSnakeCase(item)}
                  </option>
                ))}
              </select>
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium text-foreground">Account</span>
              <Input
                name="account_id"
                defaultValue={accountId}
                placeholder="actor or subject account"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium text-foreground">Proposal</span>
              <Input
                name="proposal_id"
                defaultValue={proposalId}
                placeholder="proposal id"
              />
            </label>
            <div className="flex items-end gap-2">
              <Button type="submit">
                <Search className="size-4" />
                Apply
              </Button>
              <Link
                href="/activity"
                className={buttonVariants({ variant: "outline" })}
              >
                Reset
              </Link>
            </div>
          </form>
        </CardContent>
      </Card>

      {activity.data.length === 0 ? (
        <EmptyState
          title="No activity matched these filters."
          description="Clear the filters or move back to the first cursor window."
        />
      ) : (
        <div className="grid gap-4">
          {activity.data.map((item) => (
            <Card
              key={`${item.refs.receipt_id}:${item.kind}:${item.event_type}`}
              className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]"
            >
              <CardContent className="grid gap-4 p-5 lg:grid-cols-[1fr_280px]">
                <div className="space-y-4">
                  <div className="flex flex-wrap items-center gap-2">
                    <StatusBadge value={item.kind} label={item.kind} />
                    <StatusBadge
                      value={item.success ? "true" : "false"}
                      label={item.success ? "success" : "failed"}
                    />
                    <StatusBadge
                      value={item.kind}
                      label={humanizeSnakeCase(item.event_type)}
                    />
                  </div>
                  <div className="grid gap-4 md:grid-cols-2">
                    <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
                      <p className="text-sm text-muted-foreground">Actor</p>
                      <div className="mt-2">
                        {item.actor_account_id ? (
                          <AccountLink accountId={item.actor_account_id} />
                        ) : (
                          <p className="text-sm text-foreground">—</p>
                        )}
                      </div>
                    </div>
                    <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
                      <p className="text-sm text-muted-foreground">Subject</p>
                      <div className="mt-2">
                        {item.subject_account_id ? (
                          <AccountLink accountId={item.subject_account_id} />
                        ) : item.proposal_id !== null ? (
                          <Link
                            href={`/proposals/${item.proposal_id}`}
                            className="text-sm font-medium text-foreground hover:text-[color:var(--color-primary)]"
                          >
                            Proposal #{item.proposal_id}
                          </Link>
                        ) : (
                          <p className="text-sm text-foreground">—</p>
                        )}
                      </div>
                    </div>
                  </div>
                  <details className="rounded-2xl border border-foreground/8 bg-[color:rgba(40,138,145,0.06)] p-4">
                    <summary className="cursor-pointer text-sm font-medium text-foreground">
                      Inspect payload
                    </summary>
                    <pre className="mt-3 overflow-x-auto font-mono text-[11px] leading-5 text-foreground">
                      {prettyJson(item.payload)}
                    </pre>
                  </details>
                </div>
                <div className="space-y-3 rounded-3xl border border-foreground/8 bg-[color:rgba(255,255,255,0.68)] p-5">
                  <div>
                    <p className="text-sm text-muted-foreground">Timestamp</p>
                    <p className="mt-1 text-sm text-foreground">
                      {formatTimestamp(item.refs.block_timestamp)}
                    </p>
                  </div>
                  <div>
                    <p className="text-sm text-muted-foreground">Contract</p>
                    <p className="mt-1 text-sm text-foreground">
                      {item.refs.contract_id}
                    </p>
                  </div>
                  <div>
                    <p className="text-sm text-muted-foreground">Receipt</p>
                    <p className="mt-1 font-mono text-xs text-foreground">
                      {item.refs.receipt_id}
                    </p>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      <CursorPagination
        pathname="/activity"
        searchParams={params}
        nextCursor={activity.next_cursor}
        hasMore={activity.has_more}
      />
    </div>
  );
}
