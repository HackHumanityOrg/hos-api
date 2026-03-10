import Link from "next/link";

import { AccountLink } from "@/components/venear/account-link";
import { CursorPagination } from "@/components/venear/cursor-pagination";
import { EmptyState } from "@/components/venear/empty-state";
import { MetricCard } from "@/components/venear/metric-card";
import { PageHeader } from "@/components/venear/page-header";
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
  formatTokenAmount,
  humanizeSnakeCase,
  prettyJson,
  truncateMiddle,
} from "@/lib/format";
import { readSearchParam, type SearchParamsRecord } from "@/lib/search";
import {
  type JsonValue,
  type VenearConfigSnapshotDto,
  listConfig,
  listConfigHistory,
} from "@/lib/venear-api";

type Props = {
  searchParams: Promise<SearchParamsRecord>;
};

type CouncilMember = {
  account_id: string;
  roles: string[];
  contracts: string[];
};

function addCouncilRole(
  members: Map<string, { roles: Set<string>; contracts: Set<string> }>,
  accountId: string | null | undefined,
  contractId: string,
  role: string,
): void {
  if (!accountId) {
    return;
  }

  const member = members.get(accountId) ?? {
    roles: new Set<string>(),
    contracts: new Set<string>(),
  };
  member.roles.add(role);
  member.contracts.add(contractId);
  members.set(accountId, member);
}

function buildCouncilRoster(snapshots: VenearConfigSnapshotDto[]): CouncilMember[] {
  const members = new Map<string, { roles: Set<string>; contracts: Set<string> }>();

  for (const snapshot of snapshots) {
    addCouncilRole(members, snapshot.owner_account_id, snapshot.contract_id, "owner");
    addCouncilRole(
      members,
      snapshot.proposed_new_owner_account_id,
      snapshot.contract_id,
      "proposed owner",
    );

    for (const guardian of snapshot.guardians) {
      addCouncilRole(members, guardian, snapshot.contract_id, "guardian");
    }

    for (const reviewer of snapshot.reviewer_ids) {
      addCouncilRole(members, reviewer, snapshot.contract_id, "reviewer");
    }

    for (const deployer of snapshot.lockup_code_deployers) {
      addCouncilRole(members, deployer, snapshot.contract_id, "lockup deployer");
    }
  }

  return [...members.entries()]
    .map(([account_id, member]) => ({
      account_id,
      roles: [...member.roles].sort(),
      contracts: [...member.contracts].sort(),
    }))
    .sort((left, right) => {
      const roleDelta = right.roles.length - left.roles.length;
      return roleDelta !== 0 ? roleDelta : left.account_id.localeCompare(right.account_id);
    });
}

function summarizeJsonValue(value: JsonValue): string {
  if (Array.isArray(value)) {
    return value.length === 1 ? "1 entry" : `${value.length} entries`;
  }

  if (value === null) {
    return "—";
  }

  switch (typeof value) {
    case "boolean":
      return value ? "true" : "false";
    case "number":
      return String(value);
    case "string":
      return value.length > 48 ? truncateMiddle(value, 10) : value;
    default: {
      const keys = Object.keys(value);
      return keys.length === 1 ? "1 field" : `${keys.length} fields`;
    }
  }
}

function formatDurationSeconds(value: number | null): string {
  if (value === null) {
    return "—";
  }

  const day = 86_400;
  if (value % day === 0) {
    const days = value / day;
    return days === 1 ? "1 day" : `${days} days`;
  }

  const hour = 3_600;
  if (value % hour === 0) {
    const hours = value / hour;
    return hours === 1 ? "1 hour" : `${hours} hours`;
  }

  return `${value} sec`;
}

function getExtraFields(snapshot: VenearConfigSnapshotDto): [string, JsonValue][] {
  if (
    snapshot.extra_fields === null ||
    Array.isArray(snapshot.extra_fields) ||
    typeof snapshot.extra_fields !== "object"
  ) {
    return [];
  }

  return Object.entries(snapshot.extra_fields);
}

export const dynamic = "force-dynamic";

export default async function AdminPage({ searchParams }: Props) {
  const params = await searchParams;
  const cursor = readSearchParam(params, "cursor");

  const [configResponse, configHistoryPage] = await Promise.all([
    listConfig(),
    listConfigHistory({
      limit: 16,
      cursor,
    }),
  ]);

  const visibleSnapshots = configResponse.data;
  const councilRoster = buildCouncilRoster(visibleSnapshots);
  const reviewerCount = new Set(
    visibleSnapshots.flatMap((snapshot) => snapshot.reviewer_ids),
  ).size;
  const guardianCount = new Set(
    visibleSnapshots.flatMap((snapshot) => snapshot.guardians),
  ).size;
  const latestConfigChange = configHistoryPage.data[0];

  return (
    <div className="space-y-8">
      <PageHeader
        eyebrow="Admin"
        title="Security council proposals and the veNEAR control plane."
        description="The current API exposes governance proposals through vote.dao, while the live settings and council roster span vote.dao, venear.dao, and hos-root.sputnik-dao.near."
      />

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <MetricCard
          label="Contracts in scope"
          value={String(visibleSnapshots.length)}
          supporting="The admin surface currently spans hos-root.sputnik-dao.near, venear.dao, and vote.dao."
        />
        <MetricCard
          label="Council members"
          value={String(councilRoster.length)}
          supporting="Owners, guardians, reviewers, proposed owners, and lockup deployers derived from the live config snapshots."
        />
        <MetricCard
          label="Reviewers / guardians"
          value={`${reviewerCount} / ${guardianCount}`}
          supporting="Unique reviewer and guardian accounts visible in the selected settings scope."
        />
        <MetricCard
          label="Latest settings change"
          value={
            latestConfigChange
              ? humanizeSnakeCase(latestConfigChange.field)
              : "No history"
          }
          supporting={
            latestConfigChange
              ? `${latestConfigChange.contract_id} • ${formatRelativeTime(latestConfigChange.refs.block_timestamp)}`
              : "No config history matched the current filters."
          }
          href={latestConfigChange ? "/activity?kind=config" : undefined}
        />
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Council roster</CardTitle>
          <CardDescription>
            Accounts currently referenced by ownership, guardian, reviewer, and deployer settings.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {councilRoster.length === 0 ? (
            <EmptyState
              title="No roster entries matched this contract filter."
              description="Clear the contract filter to inspect the full admin roster."
            />
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Member</TableHead>
                  <TableHead>Roles</TableHead>
                  <TableHead>Contracts</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {councilRoster.map((member) => (
                  <TableRow key={member.account_id}>
                    <TableCell className="min-w-0">
                      <AccountLink accountId={member.account_id} compact />
                    </TableCell>
                    <TableCell className="min-w-0">
                      <div className="flex flex-wrap gap-1">
                        {member.roles.map((role) => (
                          <Badge key={role} variant="outline">
                            {role}
                          </Badge>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="min-w-0">
                      <div className="flex flex-wrap gap-1">
                        {member.contracts.map((contract) => (
                          <Badge key={contract} variant="secondary">
                            {contract}
                          </Badge>
                        ))}
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Current settings</CardTitle>
          <CardDescription>
            Stable control-plane fields derived from successful governance method calls.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {visibleSnapshots.map((snapshot) => {
            const extraFields = getExtraFields(snapshot);
            const hasCouncilAssignments =
              snapshot.guardians.length > 0 ||
              snapshot.reviewer_ids.length > 0 ||
              snapshot.lockup_code_deployers.length > 0;

            return (
              <Card key={snapshot.contract_id}>
                <CardHeader className="gap-3 border-b pb-4">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="secondary">{snapshot.contract_id}</Badge>
                    {snapshot.paused !== null ? (
                      <Badge variant={snapshot.paused ? "destructive" : "outline"}>
                        {snapshot.paused ? "paused" : "live"}
                      </Badge>
                    ) : null}
                  </div>
                  <CardDescription>
                    Last touched{" "}
                    {snapshot.refs ? formatRelativeTime(snapshot.refs.block_timestamp) : "unknown"}.
                  </CardDescription>
                </CardHeader>
                <CardContent className="grid gap-4 pt-4">
                  <div className="overflow-hidden rounded-lg border">
                    <div className="grid grid-cols-[120px_minmax(0,1fr)] gap-x-3 gap-y-0 text-sm">
                      <div className="border-b bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Owner
                      </div>
                      <div className="border-b px-3 py-2">
                        {snapshot.owner_account_id ? (
                          <AccountLink accountId={snapshot.owner_account_id} compact />
                        ) : (
                          <span className="text-foreground">—</span>
                        )}
                      </div>

                      <div className="border-b bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Proposed
                      </div>
                      <div className="border-b px-3 py-2">
                        {snapshot.proposed_new_owner_account_id ? (
                          <AccountLink
                            accountId={snapshot.proposed_new_owner_account_id}
                            compact
                          />
                        ) : (
                          <span className="text-foreground">—</span>
                        )}
                      </div>

                      <div className="border-b bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Unlock
                      </div>
                      <div className="border-b px-3 py-2 font-medium text-foreground">
                        {formatDurationSeconds(snapshot.unlock_duration_sec)}
                      </div>

                      <div className="border-b bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Whitelist
                      </div>
                      <div
                        className="border-b truncate px-3 py-2 font-medium text-foreground"
                        title={snapshot.staking_pool_whitelist_account_id ?? "—"}
                      >
                        {snapshot.staking_pool_whitelist_account_id ?? "—"}
                      </div>

                      <div className="border-b bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Min deposit
                      </div>
                      <div className="border-b px-3 py-2 font-medium text-foreground">
                        {formatTokenAmount(snapshot.min_lockup_deposit)}
                      </div>

                      <div className="border-b bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Local deposit
                      </div>
                      <div className="border-b px-3 py-2 font-medium text-foreground">
                        {formatTokenAmount(snapshot.local_deposit)}
                      </div>

                      <div className="bg-muted/20 px-3 py-2 text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                        Lockup hash
                      </div>
                      <div
                        className="truncate px-3 py-2 font-mono text-xs text-foreground"
                        title={snapshot.lockup_contract_hash ?? "—"}
                      >
                        {snapshot.lockup_contract_hash
                          ? truncateMiddle(snapshot.lockup_contract_hash, 10)
                          : "—"}
                      </div>
                    </div>
                  </div>

                  <div className="flex flex-wrap gap-2">
                    <Badge variant="outline">Guardians {snapshot.guardians.length}</Badge>
                    <Badge variant="outline">Reviewers {snapshot.reviewer_ids.length}</Badge>
                    <Badge variant="outline">
                      Deployers {snapshot.lockup_code_deployers.length}
                    </Badge>
                  </div>

                  {hasCouncilAssignments ? (
                    <details className="rounded-lg border p-3">
                      <summary className="cursor-pointer text-sm font-medium text-foreground">
                        Council assignments
                      </summary>
                      <div className="mt-3 grid gap-3">
                        {snapshot.guardians.length > 0 ? (
                          <div>
                            <p className="text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                              Guardians
                            </p>
                            <div className="mt-2 flex flex-wrap gap-2">
                              {snapshot.guardians.map((guardian) => (
                                <Badge key={guardian} variant="outline" className="max-w-full">
                                  <span className="truncate" title={guardian}>
                                    {guardian}
                                  </span>
                                </Badge>
                              ))}
                            </div>
                          </div>
                        ) : null}

                        {snapshot.reviewer_ids.length > 0 ? (
                          <div>
                            <p className="text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                              Reviewers
                            </p>
                            <div className="mt-2 flex flex-wrap gap-2">
                              {snapshot.reviewer_ids.map((reviewer) => (
                                <Badge key={reviewer} variant="outline" className="max-w-full">
                                  <span className="truncate" title={reviewer}>
                                    {reviewer}
                                  </span>
                                </Badge>
                              ))}
                            </div>
                          </div>
                        ) : null}

                        {snapshot.lockup_code_deployers.length > 0 ? (
                          <div>
                            <p className="text-xs font-medium uppercase tracking-[0.18em] text-muted-foreground">
                              Lockup deployers
                            </p>
                            <div className="mt-2 flex flex-wrap gap-2">
                              {snapshot.lockup_code_deployers.map((deployer) => (
                                <Badge key={deployer} variant="outline" className="max-w-full">
                                  <span className="truncate" title={deployer}>
                                    {deployer}
                                  </span>
                                </Badge>
                              ))}
                            </div>
                          </div>
                        ) : null}
                      </div>
                    </details>
                  ) : null}

                  {extraFields.length > 0 ? (
                    <details className="rounded-lg border p-3">
                      <summary className="cursor-pointer text-sm font-medium text-foreground">
                        Extra fields
                      </summary>
                      <pre className="mt-3 overflow-x-auto font-mono text-[11px] leading-5 text-foreground">
                        {prettyJson(snapshot.extra_fields)}
                      </pre>
                    </details>
                  ) : null}
                </CardContent>
              </Card>
            );
          })}
        </CardContent>
      </Card>

      {configHistoryPage.data.length === 0 ? (
        <EmptyState
          title="No settings history matched these filters."
          description="No configuration history is available from the current cursor window."
        />
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Settings history</CardTitle>
            <CardDescription>
              Successful configuration mutations ordered by latest block height first.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table className="table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead>Change</TableHead>
                  <TableHead>Contract</TableHead>
                  <TableHead>Actor</TableHead>
                  <TableHead>Value</TableHead>
                  <TableHead>When</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {configHistoryPage.data.map((entry) => (
                  <TableRow
                    key={`${entry.refs.receipt_id}:${entry.field}:${entry.contract_id}`}
                  >
                    <TableCell className="min-w-0 w-[24%]">
                      <div className="flex min-w-0 flex-col gap-1">
                        <span className="truncate font-medium text-foreground">
                          {humanizeSnakeCase(entry.field)}
                        </span>
                        <span className="text-xs text-muted-foreground">
                          {humanizeSnakeCase(entry.method_name)}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="min-w-0">
                      <span title={entry.contract_id}>{entry.contract_id}</span>
                    </TableCell>
                    <TableCell className="min-w-0">
                      <AccountLink accountId={entry.actor_account_id} compact />
                    </TableCell>
                    <TableCell className="min-w-0">
                      <span title={prettyJson(entry.value)}>{summarizeJsonValue(entry.value)}</span>
                    </TableCell>
                    <TableCell>{formatRelativeTime(entry.refs.block_timestamp)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      <CursorPagination
        pathname="/admin"
        searchParams={params}
        nextCursor={configHistoryPage.next_cursor}
        hasMore={configHistoryPage.has_more}
      />

      <div className="flex justify-end">
        <Link
          href="/activity?kind=config"
          className={buttonVariants({ variant: "outline" })}
        >
          Config activity
        </Link>
      </div>
    </div>
  );
}
