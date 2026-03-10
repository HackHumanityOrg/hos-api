import Link from "next/link";
import { Activity, ArrowUpRight, Flag, Shield, Users } from "lucide-react";

import { MetricCard } from "@/components/venear/metric-card";
import { PageHeader } from "@/components/venear/page-header";
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { buttonVariants } from "@/components/ui/button-styles";
import {
  formatRelativeTime,
  formatTokenAmount,
  humanizeSnakeCase,
} from "@/lib/format";
import {
  listAccounts,
  listActivity,
  listConfig,
  listProposals,
} from "@/lib/venear-api";

export const dynamic = "force-dynamic";

export default async function HomePage() {
  const [proposalPage, accountPage, activityPage, configResponse] = await Promise.all([
    listProposals({ limit: 1 }),
    listAccounts({ limit: 1 }),
    listActivity({ limit: 1 }),
    listConfig(),
  ]);

  const latestProposal = proposalPage.data[0];
  const topAccount = accountPage.data[0];
  const latestActivity = activityPage.data[0];
  const uniqueGuardians = new Set(
    configResponse.data.flatMap((config) => config.guardians),
  ).size;
  const uniqueReviewers = new Set(
    configResponse.data.flatMap((config) => config.reviewer_ids),
  ).size;

  return (
    <div className="space-y-10">
      <section className="space-y-6">
        <PageHeader
          eyebrow="Canonical veNEAR"
          title="High-signal veNEAR governance."
          description="A compact entry point into proposals, accounts, activity, and admin state from the canonical `/api/v1/venear` API."
          actions={
            <>
              <Link href="/proposals" className={buttonVariants({ size: "lg" })}>
                Browse proposals
              </Link>
              <Link
                href="/admin"
                className={buttonVariants({ variant: "outline", size: "lg" })}
              >
                Open admin
              </Link>
            </>
          }
        />

        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <MetricCard
            label="Indexed contracts"
            value={String(configResponse.data.length)}
            supporting="Current governance and settings coverage in the API."
          />
          <MetricCard
            label="Latest proposal"
            value={
              latestProposal?.title ?? `Proposal #${latestProposal?.proposal_id ?? "—"}`
            }
            supporting={
              latestProposal
                ? `${humanizeSnakeCase(latestProposal.status)} • ${formatRelativeTime(latestProposal.created_at)}`
                : "No proposals in the current cursor window."
            }
            href={latestProposal ? `/proposals/${latestProposal.proposal_id}` : undefined}
          />
          <MetricCard
            label="Top voting power"
            value={topAccount ? formatTokenAmount(topAccount.effective_voting_power) : "—"}
            supporting={topAccount ? topAccount.account_id : "No accounts in the current cursor window."}
            href={topAccount ? `/accounts/${encodeURIComponent(topAccount.account_id)}` : undefined}
          />
          <MetricCard
            label="Latest activity"
            value={latestActivity ? humanizeSnakeCase(latestActivity.event_type) : "—"}
            supporting={
              latestActivity
                ? `${latestActivity.actor_account_id ?? latestActivity.refs.contract_id} • ${formatRelativeTime(latestActivity.refs.block_timestamp)}`
                : "No activity in the current cursor window."
            }
            href="/activity"
          />
        </div>
      </section>

      <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Card className="h-full">
          <CardHeader className="gap-3">
            <div className="flex size-10 items-center justify-center rounded-lg border bg-muted/30">
              <Flag className="size-5 text-primary" />
            </div>
            <CardTitle>Proposals</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col gap-2">
            <p className="text-sm text-muted-foreground">
              Track lifecycle, turnout, and vote state without the noise of the full feed.
            </p>
            <p className="text-sm font-medium text-foreground">
              {latestProposal
                ? latestProposal.title ?? `Proposal #${latestProposal.proposal_id}`
                : "No recent proposal"}
            </p>
          </CardContent>
          <CardFooter className="mt-auto">
            <Link href="/proposals" className={buttonVariants({ variant: "outline", size: "sm" })}>
              Open proposals
              <ArrowUpRight className="size-4" />
            </Link>
          </CardFooter>
        </Card>

        <Card className="h-full">
          <CardHeader className="gap-3">
            <div className="flex size-10 items-center justify-center rounded-lg border bg-muted/30">
              <Users className="size-5 text-primary" />
            </div>
            <CardTitle>Accounts</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col gap-2">
            <p className="text-sm text-muted-foreground">
              See who holds effective voting power and how participation is distributed.
            </p>
            <p className="text-sm font-medium text-foreground">
              {topAccount ? topAccount.account_id : "No account summary"}
            </p>
          </CardContent>
          <CardFooter className="mt-auto">
            <Link href="/accounts" className={buttonVariants({ variant: "outline", size: "sm" })}>
              Open accounts
              <ArrowUpRight className="size-4" />
            </Link>
          </CardFooter>
        </Card>

        <Card className="h-full">
          <CardHeader className="gap-3">
            <div className="flex size-10 items-center justify-center rounded-lg border bg-muted/30">
              <Activity className="size-5 text-primary" />
            </div>
            <CardTitle>Activity</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col gap-2">
            <p className="text-sm text-muted-foreground">
              One union feed for proposal, vote, delegation, lockup, admin, and config events.
            </p>
            <p className="text-sm font-medium text-foreground">
              {latestActivity
                ? `${humanizeSnakeCase(latestActivity.kind)} • ${humanizeSnakeCase(latestActivity.event_type)}`
                : "No recent activity"}
            </p>
          </CardContent>
          <CardFooter className="mt-auto">
            <Link href="/activity" className={buttonVariants({ variant: "outline", size: "sm" })}>
              Open activity
              <ArrowUpRight className="size-4" />
            </Link>
          </CardFooter>
        </Card>

        <Card className="h-full">
          <CardHeader className="gap-3">
            <div className="flex size-10 items-center justify-center rounded-lg border bg-muted/30">
              <Shield className="size-5 text-primary" />
            </div>
            <CardTitle>Admin</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-1 flex-col gap-2">
            <p className="text-sm text-muted-foreground">
              Inspect control-plane contracts, council roster, and current settings snapshots.
            </p>
            <p className="text-sm font-medium text-foreground">
              {configResponse.data.length} contracts • {uniqueReviewers} reviewers • {uniqueGuardians} guardians
            </p>
          </CardContent>
          <CardFooter className="mt-auto">
            <Link href="/admin" className={buttonVariants({ variant: "outline", size: "sm" })}>
              Open admin
              <ArrowUpRight className="size-4" />
            </Link>
          </CardFooter>
        </Card>
      </section>
    </div>
  );
}
