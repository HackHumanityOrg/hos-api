import Link from "next/link";
import {
  Activity,
  ArrowUpRight,
  Flag,
  Landmark,
  Shield,
  Users,
} from "lucide-react";

import { AccountLink } from "@/components/venear/account-link";
import { MetricCard } from "@/components/venear/metric-card";
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
import {
  formatRelativeTime,
  formatTimestamp,
  formatTokenAmount,
  humanizeSnakeCase,
  truncateMiddle,
} from "@/lib/format";
import {
  listAccounts,
  listActivity,
  listAdminActions,
  listConfig,
  listDelegations,
  listLockups,
  listProposals,
} from "@/lib/venear-api";
import { cn } from "@/lib/utils";

export const dynamic = "force-dynamic";

export default async function HomePage() {
  const [
    proposalPage,
    accountPage,
    activityPage,
    delegationPage,
    lockupPage,
    configResponse,
    adminPage,
  ] = await Promise.all([
    listProposals({ limit: 6 }),
    listAccounts({ limit: 6 }),
    listActivity({ limit: 8 }),
    listDelegations({ limit: 5 }),
    listLockups({ limit: 5 }),
    listConfig(),
    listAdminActions({ limit: 6 }),
  ]);

  const latestProposal = proposalPage.data[0];
  const topAccount = accountPage.data[0];
  const latestActivity = activityPage.data[0];
  const latestAdmin = adminPage.data[0];

  return (
    <div className="space-y-10">
      <section className="grid gap-6 lg:grid-cols-[1.35fr_0.9fr]">
        <div className="space-y-6">
          <PageHeader
            eyebrow="Canonical veNEAR"
            title="A Next.js surface for the new governance API."
            description="This app reads the redesigned `/api/v1/venear` contract directly. It is proposal-first where governance matters, account-first where participation matters, and explicit about lockups, delegations, config, admin history, and global activity."
            actions={
              <>
                <Link href="/proposals" className={buttonVariants({ size: "lg" })}>
                  Browse proposals
                </Link>
                <Link
                  href="/accounts"
                  className={buttonVariants({ variant: "outline", size: "lg" })}
                >
                  Explore accounts
                </Link>
              </>
            }
          />
          <div className="grid gap-4 md:grid-cols-2">
            <MetricCard
              label="Contracts indexed"
              value={String(configResponse.data.length)}
              supporting="Current config snapshots expose the live control plane across the veNEAR contracts."
            />
            <MetricCard
              label="Latest proposal"
              value={
                latestProposal?.title ?? `Proposal #${latestProposal?.proposal_id ?? "—"}`
              }
              supporting={
                latestProposal
                  ? `${humanizeSnakeCase(latestProposal.status)} • ${formatRelativeTime(latestProposal.created_at)}`
                  : "No proposals were returned from the first cursor window."
              }
              href={latestProposal ? `/proposals/${latestProposal.proposal_id}` : undefined}
            />
            <MetricCard
              label="Top current voting power"
              value={
                topAccount
                  ? formatTokenAmount(topAccount.effective_voting_power)
                  : "—"
              }
              supporting={
                topAccount
                  ? `${topAccount.account_id} leads the default account sort order.`
                  : "No accounts were returned from the first cursor window."
              }
              href={topAccount ? `/accounts/${encodeURIComponent(topAccount.account_id)}` : undefined}
            />
            <MetricCard
              label="Latest admin action"
              value={latestAdmin ? humanizeSnakeCase(latestAdmin.event_type) : "—"}
              supporting={
                latestAdmin
                  ? `${latestAdmin.contract_id} • ${formatRelativeTime(latestAdmin.refs.block_timestamp)}`
                  : "No admin actions were returned from the first cursor window."
              }
              href="/activity?kind=admin"
            />
          </div>
        </div>
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader className="gap-3 border-b border-foreground/6">
            <Badge
              variant="secondary"
              className="w-fit bg-[color:var(--color-accent)] text-foreground"
            >
              Overview
            </Badge>
            <CardTitle className="text-2xl tracking-[-0.04em]">
              What the app emphasizes
            </CardTitle>
            <CardDescription className="leading-7">
              The API is already normalized; the UI leans into that instead of recreating a compatibility layer.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-5 pt-6">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="rounded-2xl border border-foreground/8 bg-[color:rgba(40,138,145,0.08)] p-4">
                <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
                  <Flag className="size-4 text-[color:var(--color-primary)]" />
                  Proposal detail
                </div>
                <p className="text-sm leading-6 text-muted-foreground">
                  Snapshot state, tallies, lifecycle milestones, current effective votes, and full vote history.
                </p>
              </div>
              <div className="rounded-2xl border border-foreground/8 bg-[color:rgba(241,171,95,0.12)] p-4">
                <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
                  <Users className="size-4 text-[color:var(--color-primary)]" />
                  Account state
                </div>
                <p className="text-sm leading-6 text-muted-foreground">
                  Effective power, delegation posture, lockup state, participation counts, and domain timelines.
                </p>
              </div>
              <div className="rounded-2xl border border-foreground/8 bg-[color:rgba(75,192,125,0.1)] p-4">
                <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
                  <Shield className="size-4 text-[color:var(--color-primary)]" />
                  Admin control plane
                </div>
                <p className="text-sm leading-6 text-muted-foreground">
                  Current config snapshots plus the latest successful and failed operator actions.
                </p>
              </div>
              <div className="rounded-2xl border border-foreground/8 bg-[color:rgba(112,102,255,0.08)] p-4">
                <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground">
                  <Activity className="size-4 text-[color:var(--color-primary)]" />
                  Union activity feed
                </div>
                <p className="text-sm leading-6 text-muted-foreground">
                  One top-level feed for proposal, vote, delegation, lockup, reward, admin, and config events.
                </p>
              </div>
            </div>
            {latestActivity ? (
              <div className="rounded-2xl border border-foreground/8 bg-white/70 p-4">
                <div className="mb-2 flex items-center gap-2">
                  <StatusBadge value={latestActivity.kind} label={latestActivity.kind} />
                  <p className="text-sm font-medium text-foreground">
                    Latest activity pulse
                  </p>
                </div>
                <p className="text-sm leading-6 text-muted-foreground">
                  {humanizeSnakeCase(latestActivity.event_type)} on{" "}
                  {formatTimestamp(latestActivity.refs.block_timestamp)}
                  {latestActivity.actor_account_id
                    ? ` by ${latestActivity.actor_account_id}`
                    : ""}
                  .
                </p>
              </div>
            ) : null}
          </CardContent>
        </Card>
      </section>

      <section className="grid gap-6 lg:grid-cols-[1.2fr_0.8fr]">
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader className="flex-row items-center justify-between gap-4">
            <div>
              <CardTitle className="text-2xl tracking-[-0.04em]">
                Latest proposals
              </CardTitle>
              <CardDescription>
                Current summaries from the first proposal cursor window.
              </CardDescription>
            </div>
            <Link
              href="/proposals"
              className={cn(buttonVariants({ variant: "outline", size: "sm" }))}
            >
              All proposals
              <ArrowUpRight className="size-4" />
            </Link>
          </CardHeader>
          <CardContent>
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
                {proposalPage.data.map((proposal) => (
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
                          {proposal.proposer_id}
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

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader className="flex-row items-center justify-between gap-4">
            <div>
              <CardTitle className="text-2xl tracking-[-0.04em]">
                Activity pulse
              </CardTitle>
              <CardDescription>
                Normalized cross-domain items from `/activity`.
              </CardDescription>
            </div>
            <Link
              href="/activity"
              className={cn(buttonVariants({ variant: "outline", size: "sm" }))}
            >
              Full feed
              <ArrowUpRight className="size-4" />
            </Link>
          </CardHeader>
          <CardContent className="space-y-3">
            {activityPage.data.map((item) => (
              <div
                key={`${item.refs.receipt_id}:${item.kind}:${item.event_type}`}
                className="rounded-2xl border border-foreground/8 bg-white/72 p-4"
              >
                <div className="mb-2 flex flex-wrap items-center gap-2">
                  <StatusBadge value={item.kind} label={item.kind} />
                  <StatusBadge
                    value={item.success ? "true" : "false"}
                    label={item.success ? "success" : "failed"}
                  />
                </div>
                <p className="text-sm font-medium text-foreground">
                  {humanizeSnakeCase(item.event_type)}
                </p>
                <p className="mt-1 text-sm leading-6 text-muted-foreground">
                  {item.actor_account_id
                    ? `${item.actor_account_id} • `
                    : ""}
                  {formatTimestamp(item.refs.block_timestamp)}
                </p>
              </div>
            ))}
          </CardContent>
        </Card>
      </section>

      <section className="grid gap-6 lg:grid-cols-[1.15fr_0.85fr]">
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader className="flex-row items-center justify-between gap-4">
            <div>
              <CardTitle className="text-2xl tracking-[-0.04em]">
                Account state
              </CardTitle>
              <CardDescription>
                Default account ordering by effective voting power.
              </CardDescription>
            </div>
            <Link
              href="/accounts"
              className={cn(buttonVariants({ variant: "outline", size: "sm" }))}
            >
              All accounts
              <ArrowUpRight className="size-4" />
            </Link>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Account</TableHead>
                  <TableHead>Roles</TableHead>
                  <TableHead>Effective</TableHead>
                  <TableHead>Latest</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {accountPage.data.map((account) => (
                  <TableRow key={account.account_id}>
                    <TableCell className="min-w-0">
                      <AccountLink accountId={account.account_id} />
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {account.roles.map((role) => (
                          <StatusBadge
                            key={role}
                            value={role}
                            label={humanizeSnakeCase(role)}
                          />
                        ))}
                      </div>
                    </TableCell>
                    <TableCell>{formatTokenAmount(account.effective_voting_power)}</TableCell>
                    <TableCell>{formatRelativeTime(account.latest_activity_at)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>

        <div className="space-y-6">
          <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-xl tracking-[-0.04em]">
                <Users className="size-5 text-[color:var(--color-primary)]" />
                Current delegations
              </CardTitle>
              <CardDescription>
                Active delegation edges from the first cursor window.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {delegationPage.data.map((edge) => (
                <div
                  key={`${edge.delegator_account_id}:${edge.delegatee_account_id}`}
                  className="rounded-2xl border border-foreground/8 bg-white/72 p-4"
                >
                  <div className="flex items-center justify-between gap-3">
                    <AccountLink accountId={edge.delegator_account_id} compact />
                    <span className="text-xs font-semibold uppercase tracking-[0.22em] text-muted-foreground">
                      to
                    </span>
                    <AccountLink accountId={edge.delegatee_account_id} compact />
                  </div>
                  <p className="mt-3 text-sm text-muted-foreground">
                    {formatTokenAmount(edge.last_observed_delegated_voting_power)} delegated
                  </p>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-xl tracking-[-0.04em]">
                <Landmark className="size-5 text-[color:var(--color-primary)]" />
                Current lockups
              </CardTitle>
              <CardDescription>
                The newest lockup state visible through the canonical API.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {lockupPage.data.map((lockup) => (
                <div
                  key={lockup.lockup_account_id}
                  className="rounded-2xl border border-foreground/8 bg-white/72 p-4"
                >
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <p className="font-medium text-foreground">
                        {truncateMiddle(lockup.lockup_account_id, 8)}
                      </p>
                      <p className="text-sm text-muted-foreground">
                        {lockup.owner_account_id}
                      </p>
                    </div>
                    <StatusBadge
                      value={lockup.status}
                      label={humanizeSnakeCase(lockup.status)}
                    />
                  </div>
                  <p className="mt-3 text-sm text-muted-foreground">
                    {formatTokenAmount(lockup.locked_near_balance)} locked
                  </p>
                </div>
              ))}
            </CardContent>
          </Card>
        </div>
      </section>

      <section className="grid gap-6 lg:grid-cols-[0.95fr_1.05fr]">
        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-2xl tracking-[-0.04em]">
              Current config snapshots
            </CardTitle>
            <CardDescription>
              Stable fields surfaced from canonical governance method calls.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {configResponse.data.map((config) => (
              <div
                key={config.contract_id}
                className="rounded-2xl border border-foreground/8 bg-white/72 p-4"
              >
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <p className="font-medium text-foreground">{config.contract_id}</p>
                    <p className="mt-1 text-sm text-muted-foreground">
                      Owner {config.owner_account_id ?? "unknown"} • Reviewers{" "}
                      {config.reviewer_ids.length}
                    </p>
                  </div>
                  {config.paused !== null ? (
                    <StatusBadge
                      value={config.paused ? "false" : "true"}
                      label={config.paused ? "paused" : "live"}
                    />
                  ) : null}
                </div>
                <div className="mt-4 flex flex-wrap gap-2">
                  {config.guardians.slice(0, 3).map((guardian) => (
                    <Badge key={guardian} variant="outline">
                      {guardian}
                    </Badge>
                  ))}
                  {config.guardians.length > 3 ? (
                    <Badge variant="outline">
                      +{config.guardians.length - 3} guardians
                    </Badge>
                  ) : null}
                </div>
              </div>
            ))}
          </CardContent>
        </Card>

        <Card className="border-white/60 bg-white/82 shadow-[0_20px_60px_rgba(22,36,55,0.08)]">
          <CardHeader>
            <CardTitle className="text-2xl tracking-[-0.04em]">
              Latest admin actions
            </CardTitle>
            <CardDescription>
              Successful and failed operator actions from the first admin cursor window.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Action</TableHead>
                  <TableHead>Contract</TableHead>
                  <TableHead>Actor</TableHead>
                  <TableHead>When</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {adminPage.data.map((action) => (
                  <TableRow
                    key={`${action.refs.receipt_id}:${action.event_type}:${action.success}`}
                  >
                    <TableCell>
                      <div className="space-y-1">
                        <div className="flex flex-wrap gap-1">
                          <StatusBadge
                            value={action.success ? "true" : "false"}
                            label={action.success ? "success" : "failed"}
                          />
                          <StatusBadge
                            value="admin"
                            label={humanizeSnakeCase(action.event_type)}
                          />
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>{action.contract_id}</TableCell>
                    <TableCell>{action.actor_account_id ?? "—"}</TableCell>
                    <TableCell>{formatRelativeTime(action.refs.block_timestamp)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </section>
    </div>
  );
}
