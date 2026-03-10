import Link from "next/link";
import { ArrowUpRight, Search } from "lucide-react";

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
import { listAccounts } from "@/lib/venear-api";

type Props = {
  searchParams: Promise<SearchParamsRecord>;
};

const roles = ["voter", "delegator", "delegatee", "reviewer", "admin", "proposer"];

export default async function AccountsPage({ searchParams }: Props) {
  const params = await searchParams;
  const q = readSearchParam(params, "q");
  const role = readSearchParam(params, "role");
  const cursor = readSearchParam(params, "cursor");

  const accounts = await listAccounts({
    limit: 20,
    cursor,
    q,
    role,
  });

  return (
    <div className="space-y-8">
      <PageHeader
        eyebrow="Accounts"
        title="Current governance state per account."
        description="Accounts unify voting power, delegation posture, lockup presence, participation counts, and latest governance activity. Detail pages then branch into the domain-specific histories."
      />

      <Card>
        <CardHeader>
          <CardTitle>Filters</CardTitle>
          <CardDescription>
            Query account summaries by role or free-text account match.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="grid items-end gap-3 md:grid-cols-[1fr_220px_auto]">
            <label className="grid gap-2">
              <span className="text-sm font-medium text-foreground">Search</span>
              <Input
                name="q"
                defaultValue={q}
                placeholder="account.near"
              />
            </label>
            <label className="grid gap-2">
              <span className="text-sm font-medium text-foreground">Role</span>
              <select
                name="role"
                defaultValue={role ?? ""}
                className="block h-8 w-full rounded-lg border border-input bg-background px-2.5 text-sm outline-none transition-colors focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              >
                <option value="">All roles</option>
                {roles.map((item) => (
                  <option key={item} value={item}>
                    {humanizeSnakeCase(item)}
                  </option>
                ))}
              </select>
            </label>
            <div className="flex flex-wrap gap-2 md:self-end">
              <Button type="submit">
                <Search className="size-4" />
                Apply
              </Button>
              <Link
                href="/accounts"
                className={buttonVariants({ variant: "outline" })}
              >
                Reset
              </Link>
            </div>
          </form>
        </CardContent>
      </Card>

      {accounts.data.length === 0 ? (
        <EmptyState
          title="No accounts matched these filters."
          description="Clear the current filters or move back to the first cursor window."
        />
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Account summaries</CardTitle>
            <CardDescription>
              Sorted by effective voting power descending, then account id.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Account</TableHead>
                  <TableHead>Roles</TableHead>
                  <TableHead>Effective</TableHead>
                  <TableHead>Delegation</TableHead>
                  <TableHead>Lockup</TableHead>
                  <TableHead>Latest activity</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {accounts.data.map((account) => (
                  <TableRow key={account.account_id}>
                    <TableCell className="min-w-0">
                      <AccountLink accountId={account.account_id} />
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {account.roles.map((item) => (
                          <StatusBadge
                            key={item}
                            value={item}
                            label={humanizeSnakeCase(item)}
                          />
                        ))}
                      </div>
                    </TableCell>
                    <TableCell>{formatTokenAmount(account.effective_voting_power)}</TableCell>
                    <TableCell>
                      {account.delegated_to_account_id
                        ? `to ${account.delegated_to_account_id}`
                        : account.inbound_delegator_count > 0
                          ? `${account.inbound_delegator_count} inbound`
                          : "—"}
                    </TableCell>
                    <TableCell>{account.has_lockup ? "Yes" : "No"}</TableCell>
                    <TableCell>{formatRelativeTime(account.latest_activity_at)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      <CursorPagination
        pathname="/accounts"
        searchParams={params}
        nextCursor={accounts.next_cursor}
        hasMore={accounts.has_more}
      />

      <div className="flex justify-end">
        <Link
          href="/activity?kind=vote"
          className={buttonVariants({ variant: "outline" })}
        >
          Vote activity
          <ArrowUpRight className="size-4" />
        </Link>
      </div>
    </div>
  );
}
