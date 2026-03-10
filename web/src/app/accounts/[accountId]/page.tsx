import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import {
  ArrowUpRight,
  Coins,
  GitBranch,
  History,
  Shield,
  Vote,
} from "lucide-react";

import { AccountLink } from "@/components/venear/account-link";
import { EmptyState } from "@/components/venear/empty-state";
import { AccountVotingPowerChart } from "@/components/venear/account-voting-power-chart";
import { PageHeader } from "@/components/venear/page-header";
import { StatusBadge } from "@/components/venear/status-badge";
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
  formatRelativeTime,
  formatTimestamp,
  formatTokenAmount,
  humanizeSnakeCase,
  prettyJson,
  truncateMiddle,
} from "@/lib/format";
import {
  getAccount,
  isVenearApiError,
  listAccountDelegationHistory,
  listAccountLockupEvents,
  listAccountProposals,
  listAccountRewardsEvents,
  listAccountVoteHistory,
  listAccountVotes,
  listAccountVotingPowerHistory,
} from "@/lib/venear-api";

type Props = {
  params: Promise<{ accountId: string }>;
};

async function loadAccountPageData(accountId: string) {
  try {
    const [account, proposals, votes, voteHistory, votingPowerHistory, delegationHistory, lockupEvents, rewardEvents] =
      await Promise.all([
        getAccount(accountId),
        listAccountProposals(accountId, { limit: 10 }),
        listAccountVotes(accountId, { limit: 10 }),
        listAccountVoteHistory(accountId, { limit: 10 }),
        listAccountVotingPowerHistory(accountId, { limit: 40 }),
        listAccountDelegationHistory(accountId, { limit: 10 }),
        listAccountLockupEvents(accountId, { limit: 10 }),
        listAccountRewardsEvents(accountId, { limit: 10 }),
      ]);

    return {
      account,
      proposals,
      votes,
      voteHistory,
      votingPowerHistory,
      delegationHistory,
      lockupEvents,
      rewardEvents,
    };
  } catch (error) {
    if (isVenearApiError(error) && error.status === 404) {
      notFound();
    }

    throw error;
  }
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { accountId: rawAccountId } = await params;
  const accountId = decodeURIComponent(rawAccountId);

  try {
    const account = await getAccount(accountId);
    return {
      title: account.account_id,
      description: `Canonical veNEAR account state for ${account.account_id}.`,
    };
  } catch {
    return {
      title: accountId,
    };
  }
}

export default async function AccountDetailPage({ params }: Props) {
  const { accountId: rawAccountId } = await params;
  const accountId = decodeURIComponent(rawAccountId);
  const {
    account,
    proposals,
    votes,
    voteHistory,
    votingPowerHistory,
    delegationHistory,
    lockupEvents,
    rewardEvents,
  } = await loadAccountPageData(accountId);

  return (
    <div className="space-y-8">
      <PageHeader
        eyebrow="Account state"
        title={account.account_id}
        description="This account detail combines current governance posture with separate timelines for proposals, votes, voting-power mutations, delegations, lockups, and rewards."
        actions={
          <Link
            href={`/accounts?q=${encodeURIComponent(account.account_id)}`}
            className={buttonVariants({ variant: "outline", size: "lg" })}
          >
            Back to accounts
            <ArrowUpRight className="size-4" />
          </Link>
        }
      />

      <div className="flex flex-wrap items-center gap-2">
        {account.roles.map((role) => (
          <StatusBadge key={role} value={role} label={humanizeSnakeCase(role)} />
        ))}
      </div>

      <section className="grid gap-4 lg:grid-cols-4">
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Effective power</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            <p className="text-2xl font-semibold tracking-[-0.04em] text-foreground">
              {formatTokenAmount(account.effective_voting_power)}
            </p>
            <p className="text-sm text-muted-foreground">
              Own {formatTokenAmount(account.own_voting_power)} • Delegated{" "}
              {formatTokenAmount(account.delegated_voting_power)}
            </p>
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Delegation posture</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm">
            <p className="text-muted-foreground">
              Delegated to {account.delegation.delegated_to_account_id ?? "nobody"}
            </p>
            <p className="text-muted-foreground">
              {account.delegation.inbound_delegator_count} inbound delegators
            </p>
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Current lockup</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm">
            {account.lockup ? (
              <>
                <StatusBadge
                  value={account.lockup.status}
                  label={humanizeSnakeCase(account.lockup.status)}
                />
                <p className="text-muted-foreground">
                  {formatTokenAmount(account.lockup.locked_near_balance)}
                </p>
                <p className="text-muted-foreground">
                  Updated {formatRelativeTime(account.lockup.last_updated_at)}
                </p>
              </>
            ) : (
              <p className="text-muted-foreground">
                No current lockup is attached to this account.
              </p>
            )}
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-lg">Participation</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 text-sm">
            <p className="text-muted-foreground">
              {account.participation.proposals_created_count} created proposals
            </p>
            <p className="text-muted-foreground">
              {account.participation.current_effective_votes_count} current effective votes
            </p>
            <p className="text-muted-foreground">
              {account.participation.vote_events_count} vote events total
            </p>
          </CardContent>
        </Card>
      </section>

      <section className="grid gap-4 lg:grid-cols-[0.9fr_1.1fr]">
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-xl tracking-[-0.04em]">
              Governance state
            </CardTitle>
            <CardDescription>
              Current summary fields directly returned by the account detail endpoint.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4 text-sm">
            <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
              <p className="text-muted-foreground">Latest activity</p>
              <p className="mt-1 text-foreground">
                {formatTimestamp(account.latest_activity_at)}
              </p>
            </div>
            <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
              <p className="text-muted-foreground">Latest refs</p>
              <p className="mt-1 font-mono text-xs text-foreground">
                {account.latest_refs
                  ? truncateMiddle(account.latest_refs.receipt_id, 10)
                  : "—"}
              </p>
            </div>
            {account.governance_state ? (
              <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
                <p className="text-muted-foreground">Governance counters</p>
                <p className="mt-1 text-foreground">
                  {account.governance_state.proposals_created_count} created •{" "}
                  {account.governance_state.proposals_reviewed_count} reviewed •{" "}
                  {account.governance_state.current_effective_votes_count} effective votes
                </p>
              </div>
            ) : null}
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-xl tracking-[-0.04em]">
              Current edges
            </CardTitle>
            <CardDescription>
              Direct links that describe how this account participates right now.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4 md:grid-cols-2">
            <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
              <p className="text-sm text-muted-foreground">Outbound delegation</p>
              {account.delegation.delegated_to_account_id ? (
                <div className="mt-2">
                  <AccountLink accountId={account.delegation.delegated_to_account_id} />
                </div>
              ) : (
                <p className="mt-2 text-sm text-foreground">No active outbound delegation.</p>
              )}
            </div>
            <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4">
              <p className="text-sm text-muted-foreground">Inbound delegation</p>
              <p className="mt-2 text-sm text-foreground">
                {account.delegation.inbound_delegator_count} active delegators
              </p>
            </div>
            <div className="rounded-2xl border border-foreground/8 bg-white/72 p-4 md:col-span-2">
              <p className="text-sm text-muted-foreground">Current lockup</p>
              <p className="mt-2 text-sm text-foreground">
                {account.lockup
                  ? `${account.lockup.lockup_account_id} • ${formatTokenAmount(account.lockup.locked_near_balance)}`
                  : "No current lockup"}
              </p>
            </div>
          </CardContent>
        </Card>
      </section>

      <Tabs defaultValue="proposals" className="gap-4">
        <TabsList
          variant="line"
          className="h-auto w-full flex-wrap justify-start gap-2 [&_[data-slot=tabs-trigger]]:flex-none"
        >
          <TabsTrigger value="proposals">
            <Vote className="size-4" />
            Proposals
          </TabsTrigger>
          <TabsTrigger value="votes">
            <Vote className="size-4" />
            Votes
          </TabsTrigger>
          <TabsTrigger value="power">
            <Coins className="size-4" />
            Voting power
          </TabsTrigger>
          <TabsTrigger value="delegations">
            <GitBranch className="size-4" />
            Delegations
          </TabsTrigger>
          <TabsTrigger value="lockups">
            <Shield className="size-4" />
            Lockups
          </TabsTrigger>
          <TabsTrigger value="rewards">
            <History className="size-4" />
            Rewards
          </TabsTrigger>
        </TabsList>

        <TabsContent value="proposals">
          {proposals.data.length === 0 ? (
            <EmptyState
              title="No related proposals."
              description="This account does not currently appear on proposal summaries in the first account-scoped cursor window."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Proposal</TableHead>
                      <TableHead>Status</TableHead>
                      <TableHead>Turnout</TableHead>
                      <TableHead>Created</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {proposals.data.map((proposal) => (
                      <TableRow key={proposal.proposal_id}>
                        <TableCell className="min-w-0">
                          <Link
                            href={`/proposals/${proposal.proposal_id}`}
                            className="block min-w-0"
                          >
                            <div className="truncate font-medium text-foreground">
                              {proposal.title ?? `Proposal #${proposal.proposal_id}`}
                            </div>
                            <div className="truncate text-xs text-muted-foreground">
                              {proposal.contract_id}
                            </div>
                          </Link>
                        </TableCell>
                        <TableCell>
                          <StatusBadge
                            value={proposal.status}
                            label={humanizeSnakeCase(proposal.status)}
                          />
                        </TableCell>
                        <TableCell>{formatTokenAmount(proposal.turnout_voting_power)}</TableCell>
                        <TableCell>{formatRelativeTime(proposal.created_at)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="votes">
          <div className="space-y-4">
            {votes.data.length === 0 ? (
              <EmptyState
                title="No current effective votes."
                description="This account currently has no effective votes."
              />
            ) : (
              <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
                <CardHeader>
                  <CardTitle>Current votes</CardTitle>
                  <CardDescription>
                    Current effective votes across proposals.
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Proposal</TableHead>
                        <TableHead>Option</TableHead>
                        <TableHead>Voting power</TableHead>
                        <TableHead>When</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {votes.data.map((vote) => (
                        <TableRow key={`${vote.proposal_id}:${vote.refs.receipt_id}`}>
                          <TableCell>
                            <Link
                              href={`/proposals/${vote.proposal_id}`}
                              className="font-medium text-foreground hover:text-[color:var(--color-primary)]"
                            >
                              {vote.proposal_title ?? `Proposal #${vote.proposal_id}`}
                            </Link>
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

            {voteHistory.data.length > 0 ? (
              <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
                <CardHeader>
                  <CardTitle>Vote history</CardTitle>
                  <CardDescription>
                    Successful vote changes scoped to this account.
                  </CardDescription>
                </CardHeader>
                <CardContent>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Proposal</TableHead>
                        <TableHead>Event</TableHead>
                        <TableHead>Power</TableHead>
                        <TableHead>Effective</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {voteHistory.data.map((event) => (
                        <TableRow key={`${event.proposal_id}:${event.refs.receipt_id}:${event.event_type}`}>
                          <TableCell>
                            <Link
                              href={`/proposals/${event.proposal_id}`}
                              className="font-medium text-foreground hover:text-[color:var(--color-primary)]"
                            >
                              Proposal #{event.proposal_id}
                            </Link>
                          </TableCell>
                          <TableCell>
                            <StatusBadge
                              value="vote"
                              label={humanizeSnakeCase(event.event_type)}
                            />
                          </TableCell>
                          <TableCell>{formatTokenAmount(event.voting_power)}</TableCell>
                          <TableCell>{event.effective_after_event ? "Yes" : "No"}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>
            ) : null}
          </div>
        </TabsContent>

        <TabsContent value="power">
          {votingPowerHistory.data.length === 0 ? (
            <EmptyState
              title="No voting-power mutations."
              description="Derived mutations would appear here when the canonical ledger changes."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardHeader>
                <CardTitle>Voting power curve</CardTitle>
                <CardDescription>
                  Effective voting power after each mutation in the latest history window.
                </CardDescription>
              </CardHeader>
              <CardContent>
                <AccountVotingPowerChart events={votingPowerHistory.data} />
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="delegations">
          {delegationHistory.data.length === 0 ? (
            <EmptyState
              title="No delegation history."
              description="This account exists, but there are no delegation events in the current account-scoped cursor window."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Direction</TableHead>
                      <TableHead>Event</TableHead>
                      <TableHead>Counterparty</TableHead>
                      <TableHead>Power</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {delegationHistory.data.map((event) => (
                      <TableRow key={`${event.refs.receipt_id}:${event.direction}:${event.event_type}`}>
                        <TableCell>{humanizeSnakeCase(event.direction)}</TableCell>
                        <TableCell>{humanizeSnakeCase(event.event_type)}</TableCell>
                        <TableCell>
                          {event.counterparty_account_id ? (
                            <AccountLink accountId={event.counterparty_account_id} compact />
                          ) : (
                            "—"
                          )}
                        </TableCell>
                        <TableCell>{formatTokenAmount(event.delegated_voting_power)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="lockups">
          {lockupEvents.data.length === 0 ? (
            <EmptyState
              title="No lockup events."
              description="This account exists, but there are no lockup lifecycle events in the current account-scoped cursor window."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Lockup</TableHead>
                      <TableHead>Event</TableHead>
                      <TableHead>Balance</TableHead>
                      <TableHead>Payload</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {lockupEvents.data.map((event) => (
                      <TableRow key={`${event.lockup_account_id}:${event.refs.receipt_id}:${event.event_type}`}>
                        <TableCell>{event.lockup_account_id}</TableCell>
                        <TableCell>{humanizeSnakeCase(event.event_type)}</TableCell>
                        <TableCell>{formatTokenAmount(event.locked_near_balance)}</TableCell>
                        <TableCell className="min-w-0">
                          <details className="text-xs text-muted-foreground">
                            <summary className="cursor-pointer">Inspect</summary>
                            <pre className="mt-2 overflow-x-auto rounded-xl bg-[color:rgba(40,138,145,0.06)] p-3 font-mono text-[11px] leading-5">
                              {prettyJson(event.payload)}
                            </pre>
                          </details>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>

        <TabsContent value="rewards">
          {rewardEvents.data.length === 0 ? (
            <EmptyState
              title="No reward events."
              description="Reward and claim activity would appear here when present."
            />
          ) : (
            <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
              <CardContent className="pt-6">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Event</TableHead>
                      <TableHead>Amount</TableHead>
                      <TableHead>When</TableHead>
                      <TableHead>Payload</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rewardEvents.data.map((event) => (
                      <TableRow key={`${event.refs.receipt_id}:${event.event_type}`}>
                        <TableCell>{humanizeSnakeCase(event.event_type)}</TableCell>
                        <TableCell>{formatTokenAmount(event.amount)}</TableCell>
                        <TableCell>{formatTimestamp(event.refs.block_timestamp)}</TableCell>
                        <TableCell>
                          <details className="text-xs text-muted-foreground">
                            <summary className="cursor-pointer">Inspect</summary>
                            <pre className="mt-2 overflow-x-auto rounded-xl bg-[color:rgba(40,138,145,0.06)] p-3 font-mono text-[11px] leading-5">
                              {prettyJson(event.payload)}
                            </pre>
                          </details>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>
          )}
        </TabsContent>
      </Tabs>
    </div>
  );
}
