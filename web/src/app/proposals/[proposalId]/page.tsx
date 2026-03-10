import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { ArrowUpRight, GitBranch, ScrollText, Vote } from "lucide-react";

import { AccountLink } from "@/components/venear/account-link";
import { EmptyState } from "@/components/venear/empty-state";
import { PageHeader } from "@/components/venear/page-header";
import { StatusBadge } from "@/components/venear/status-badge";
import { Badge } from "@/components/ui/badge";
import { buttonVariants } from "@/components/ui/button-styles";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  formatTimestamp,
  formatTokenAmount,
  humanizeSnakeCase,
  percentageFromRaw,
  prettyJson,
  truncateMiddle,
} from "@/lib/format";
import {
  getProposal,
  isVenearApiError,
  listProposalTimeline,
  listProposalVoteHistory,
  listProposalVotes,
} from "@/lib/venear-api";

type Props = {
  params: Promise<{ proposalId: string }>;
};

async function loadProposalPageData(proposalId: number) {
  try {
    const [proposal, timeline, votes, voteHistory] = await Promise.all([
      getProposal(proposalId),
      listProposalTimeline(proposalId, { limit: 20 }),
      listProposalVotes(proposalId, { limit: 20 }),
      listProposalVoteHistory(proposalId, { limit: 20 }),
    ]);

    return { proposal, timeline, votes, voteHistory };
  } catch (error) {
    if (isVenearApiError(error) && error.status === 404) {
      notFound();
    }

    throw error;
  }
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { proposalId } = await params;
  const numericProposalId = Number(proposalId);

  if (!Number.isInteger(numericProposalId)) {
    return {
      title: "Proposal not found",
    };
  }

  try {
    const proposal = await getProposal(numericProposalId);
    return {
      title: proposal.title ?? `Proposal #${proposal.proposal_id}`,
      description:
        proposal.description ??
        `Canonical veNEAR proposal ${proposal.proposal_id} detail.`,
    };
  } catch {
    return {
      title: `Proposal #${proposalId}`,
    };
  }
}

export default async function ProposalDetailPage({ params }: Props) {
  const { proposalId } = await params;
  const numericProposalId = Number(proposalId);

  if (!Number.isInteger(numericProposalId)) {
    notFound();
  }

  const { proposal, timeline, votes, voteHistory } =
    await loadProposalPageData(numericProposalId);

  return (
    <div className="space-y-8">
      <PageHeader
        eyebrow={`Proposal #${proposal.proposal_id}`}
        title={proposal.title ?? "Untitled proposal"}
        description={
          proposal.description ??
          "This proposal does not expose a description string in the indexed metadata."
        }
        actions={
          proposal.link ? (
            <Link
              href={proposal.link}
              target="_blank"
              rel="noreferrer"
              className={buttonVariants({ variant: "outline", size: "lg" })}
            >
              Open proposal link
              <ArrowUpRight className="size-4" />
            </Link>
          ) : null
        }
      />

      <div className="flex flex-wrap items-center gap-2">
        <StatusBadge value={proposal.status} label={humanizeSnakeCase(proposal.status)} />
        <Badge variant="outline">{proposal.contract_id}</Badge>
        <Badge variant="outline">
          Latest {humanizeSnakeCase(proposal.latest_event_type)}
        </Badge>
      </div>

      <section className="grid gap-4 lg:grid-cols-4">
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Participants</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3 text-sm">
            <div>
              <p className="text-muted-foreground">Proposer</p>
              <AccountLink accountId={proposal.proposer_id} />
            </div>
            <div>
              <p className="text-muted-foreground">Reviewer</p>
              {proposal.reviewer_id ? (
                <AccountLink accountId={proposal.reviewer_id} />
              ) : (
                <p className="text-foreground">—</p>
              )}
            </div>
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Turnout</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <p className="text-2xl font-semibold tracking-[-0.04em] text-foreground">
              {formatTokenAmount(proposal.turnout_voting_power)}
            </p>
            <p className="text-sm text-muted-foreground">
              {proposal.turnout_voter_count} voters currently affect the tally.
            </p>
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Snapshot</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm">
            {proposal.snapshot ? (
              <>
                <p className="text-2xl font-semibold tracking-[-0.04em] text-foreground">
                  {formatTokenAmount(proposal.snapshot.total_venear)}
                </p>
                <p className="text-muted-foreground">
                  Block {proposal.snapshot.block_height ?? "—"} •{" "}
                  {proposal.snapshot.root
                    ? truncateMiddle(proposal.snapshot.root, 8)
                    : "no root"}
                </p>
              </>
            ) : (
              <p className="text-sm leading-6 text-muted-foreground">
                No finalized snapshot has been attached to this proposal yet.
              </p>
            )}
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Window</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm">
            <p className="text-muted-foreground">
              Created {formatTimestamp(proposal.milestones.created_at)}
            </p>
            <p className="text-muted-foreground">
              Voting start {formatTimestamp(proposal.milestones.voting_start_at)}
            </p>
            <p className="text-muted-foreground">
              Voting end {formatTimestamp(proposal.milestones.voting_end_at)}
            </p>
          </CardContent>
        </Card>
      </section>

      <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
        <CardHeader>
          <CardTitle className="text-2xl tracking-[-0.04em]">
            Tally by option
          </CardTitle>
          <CardDescription>
            All proposal options are rendered, including zero-vote options.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {proposal.tally.map((row) => (
            <div key={row.option_index} className="space-y-2">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <p className="font-medium text-foreground">
                    {row.option_label ?? `Option ${row.option_index + 1}`}
                  </p>
                  <p className="text-sm text-muted-foreground">
                    {row.votes_count} votes
                  </p>
                </div>
                <div className="text-right">
                  <p className="font-medium text-foreground">
                    {formatTokenAmount(row.voting_power)}
                  </p>
                  <p className="text-sm text-muted-foreground">
                    {percentageFromRaw(
                      row.voting_power.raw,
                      proposal.turnout_voting_power.raw,
                    )}
                  </p>
                </div>
              </div>
              <div className="h-2 rounded-full bg-[color:rgba(40,138,145,0.08)]">
                <div
                  className="h-2 rounded-full bg-[color:var(--color-primary)]"
                  style={{
                    width: percentageFromRaw(
                      row.voting_power.raw,
                      proposal.turnout_voting_power.raw,
                    ),
                  }}
                />
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <Tabs defaultValue="timeline" className="gap-4">
        <TabsList variant="line" className="w-full justify-start gap-2 overflow-x-auto">
          <TabsTrigger value="timeline">
            <GitBranch className="size-4" />
            Timeline
          </TabsTrigger>
          <TabsTrigger value="votes">
            <Vote className="size-4" />
            Current votes
          </TabsTrigger>
          <TabsTrigger value="history">
            <ScrollText className="size-4" />
            Vote history
          </TabsTrigger>
          <TabsTrigger value="metadata">
            <ScrollText className="size-4" />
            Metadata
          </TabsTrigger>
        </TabsList>

        <TabsContent value="timeline">
          {timeline.data.length === 0 ? (
            <EmptyState
              title="No lifecycle events yet."
              description="Successful state-changing lifecycle events would appear here."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Event</TableHead>
                      <TableHead>Actor</TableHead>
                      <TableHead>Status after</TableHead>
                      <TableHead>When</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {timeline.data.map((event) => (
                      <TableRow key={`${event.refs.receipt_id}:${event.event_type}`}>
                        <TableCell className="min-w-0">
                          <div className="space-y-2">
                            <StatusBadge
                              value="proposal"
                              label={humanizeSnakeCase(event.event_type)}
                            />
                            <details className="text-xs text-muted-foreground">
                              <summary className="cursor-pointer">
                                Inspect payload
                              </summary>
                              <pre className="mt-2 overflow-x-auto rounded-xl bg-[color:rgba(40,138,145,0.06)] p-3 font-mono text-[11px] leading-5">
                                {prettyJson(event.payload)}
                              </pre>
                            </details>
                          </div>
                        </TableCell>
                        <TableCell>
                          {event.actor_account_id ? (
                            <AccountLink accountId={event.actor_account_id} compact />
                          ) : (
                            "—"
                          )}
                        </TableCell>
                        <TableCell>{humanizeSnakeCase(event.status_after_event)}</TableCell>
                        <TableCell>{formatTimestamp(event.refs.block_timestamp)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="votes">
          {votes.data.length === 0 ? (
            <EmptyState
              title="No effective votes."
              description="This proposal currently has no effective votes."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Voter</TableHead>
                      <TableHead>Option</TableHead>
                      <TableHead>Voting power</TableHead>
                      <TableHead>When</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {votes.data.map((vote) => (
                      <TableRow key={`${vote.voter_account_id}:${vote.refs.receipt_id}`}>
                        <TableCell>
                          <AccountLink accountId={vote.voter_account_id} compact />
                        </TableCell>
                        <TableCell>{vote.vote_label ?? `Option ${vote.vote_option + 1}`}</TableCell>
                        <TableCell>{formatTokenAmount(vote.voting_power)}</TableCell>
                        <TableCell>{formatTimestamp(vote.voted_at)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="history">
          {voteHistory.data.length === 0 ? (
            <EmptyState
              title="No vote history."
              description="Successful vote changes would appear here."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Voter</TableHead>
                      <TableHead>Event</TableHead>
                      <TableHead>Voting power</TableHead>
                      <TableHead>Effective after</TableHead>
                      <TableHead>When</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {voteHistory.data.map((event) => (
                      <TableRow
                        key={`${event.voter_account_id}:${event.refs.receipt_id}:${event.event_type}`}
                      >
                        <TableCell>
                          <AccountLink accountId={event.voter_account_id} compact />
                        </TableCell>
                        <TableCell>
                          <div className="space-y-1">
                            <StatusBadge
                              value="vote"
                              label={humanizeSnakeCase(event.event_type)}
                            />
                            <p className="text-xs text-muted-foreground">
                              {event.vote_label ??
                                (event.vote_option !== null
                                  ? `Option ${event.vote_option + 1}`
                                  : "No option")}
                            </p>
                          </div>
                        </TableCell>
                        <TableCell>{formatTokenAmount(event.voting_power)}</TableCell>
                        <TableCell>{event.effective_after_event ? "Yes" : "No"}</TableCell>
                        <TableCell>{formatTimestamp(event.refs.block_timestamp)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="metadata">
          <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
            <CardHeader>
              <CardTitle>Canonical metadata and refs</CardTitle>
              <CardDescription>
                Embedded metadata and provenance references from the API.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid gap-4 md:grid-cols-2">
                <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4 text-sm">
                  <p className="text-muted-foreground">Receipt</p>
                  <p className="mt-1 font-mono text-xs text-foreground">
                    {truncateMiddle(proposal.refs.receipt_id, 10)}
                  </p>
                </div>
                <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4 text-sm">
                  <p className="text-muted-foreground">Transaction hash</p>
                  <p className="mt-1 font-mono text-xs text-foreground">
                    {proposal.refs.transaction_hash
                      ? truncateMiddle(proposal.refs.transaction_hash, 10)
                      : "—"}
                  </p>
                </div>
              </div>
              <pre className="overflow-x-auto rounded-2xl bg-[color:rgba(40,138,145,0.06)] p-4 font-mono text-xs leading-6 text-foreground">
                {prettyJson(proposal.metadata)}
              </pre>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
