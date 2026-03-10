import Link from "next/link";
import { ArrowUpRight, Search } from "lucide-react";

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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  formatRelativeTime,
  formatTokenAmount,
  humanizeSnakeCase,
} from "@/lib/format";
import { readSearchParam, type SearchParamsRecord } from "@/lib/search";
import { listProposals } from "@/lib/venear-api";

type Props = {
  searchParams: Promise<SearchParamsRecord>;
};

const proposalStatuses = ["created", "approved", "voting", "finished", "rejected"];

export default async function ProposalsPage({ searchParams }: Props) {
  const params = await searchParams;
  const q = readSearchParam(params, "q");
  const status = readSearchParam(params, "status");
  const cursor = readSearchParam(params, "cursor");

  const proposals = await listProposals({
    limit: 20,
    cursor,
    q,
    status,
  });

  return (
    <div className="space-y-8">
      <PageHeader
        eyebrow="Proposals"
        title="Lifecycle, turnout, and current vote state."
        description="The proposal surface exposes canonical lifecycle milestones, nullable snapshots, current tallies, and direct entry points into the proposal-specific timeline and vote state."
      />

      <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
        <CardHeader>
          <CardTitle>Filters</CardTitle>
          <CardDescription>
            Query proposal summaries by status or free-text match.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="grid gap-3 md:grid-cols-[1fr_200px_auto]">
            <label className="space-y-2">
              <span className="text-sm font-medium text-foreground">Search</span>
              <Input
                name="q"
                defaultValue={q}
                placeholder="Title, description, link, proposer"
              />
            </label>
            <label className="space-y-2">
              <span className="text-sm font-medium text-foreground">Status</span>
              <select
                name="status"
                defaultValue={status ?? ""}
                className="h-8 rounded-lg border border-input bg-white/90 px-2.5 text-sm outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              >
                <option value="">All statuses</option>
                {proposalStatuses.map((item) => (
                  <option key={item} value={item}>
                    {humanizeSnakeCase(item)}
                  </option>
                ))}
              </select>
            </label>
            <div className="flex items-end gap-2">
              <Button type="submit">
                <Search className="size-4" />
                Apply
              </Button>
              <Link
                href="/proposals"
                className={buttonVariants({ variant: "outline" })}
              >
                Reset
              </Link>
            </div>
          </form>
        </CardContent>
      </Card>

      {proposals.data.length === 0 ? (
        <EmptyState
          title="No proposals matched these filters."
          description="Clear the current filters or move back to the first cursor window."
        />
      ) : (
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle>Proposal summaries</CardTitle>
            <CardDescription>
              Sorted by latest event timestamp descending.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Proposal</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Proposer</TableHead>
                  <TableHead>Turnout</TableHead>
                  <TableHead>Latest event</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {proposals.data.map((proposal) => (
                  <TableRow key={proposal.proposal_id}>
                    <TableCell className="min-w-0">
                      <Link
                        href={`/proposals/${proposal.proposal_id}`}
                        className="block min-w-0 space-y-1"
                      >
                        <div className="truncate font-medium text-foreground">
                          {proposal.title ?? `Proposal #${proposal.proposal_id}`}
                        </div>
                        <div className="truncate text-xs text-muted-foreground">
                          {proposal.description ?? proposal.link ?? proposal.contract_id}
                        </div>
                      </Link>
                    </TableCell>
                    <TableCell>
                      <StatusBadge
                        value={proposal.status}
                        label={humanizeSnakeCase(proposal.status)}
                      />
                    </TableCell>
                    <TableCell>{proposal.proposer_id}</TableCell>
                    <TableCell>{formatTokenAmount(proposal.turnout_voting_power)}</TableCell>
                    <TableCell>
                      <div className="space-y-1">
                        <StatusBadge
                          value="proposal"
                          label={humanizeSnakeCase(proposal.latest_event_type)}
                        />
                        <p className="text-xs text-muted-foreground">
                          {formatRelativeTime(proposal.refs.block_timestamp)}
                        </p>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      <CursorPagination
        pathname="/proposals"
        searchParams={params}
        nextCursor={proposals.next_cursor}
        hasMore={proposals.has_more}
      />

      <div className="flex justify-end">
        <Link
          href="/activity?kind=proposal"
          className={buttonVariants({ variant: "outline" })}
        >
          Proposal activity
          <ArrowUpRight className="size-4" />
        </Link>
      </div>
    </div>
  );
}
