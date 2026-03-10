"use client";

import Link from "next/link";
import { Activity, ArrowUpRight, TrendingUp } from "lucide-react";
import { Area, AreaChart, CartesianGrid, XAxis, YAxis } from "recharts";

import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  formatTimestamp,
  formatTokenAmount,
  humanizeSnakeCase,
} from "@/lib/format";
import type {
  VenearAccountVotingPowerEventDto,
  VenearTokenAmountDto,
} from "@/lib/venear-api";

const chartConfig = {
  effective_voting_power: {
    label: "Effective voting power",
    color: "var(--color-primary)",
  },
} satisfies ChartConfig;

const axisDateFormatter = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
});

const tooltipDateFormatter = new Intl.DateTimeFormat("en-US", {
  month: "short",
  day: "numeric",
  year: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

const compactNumberFormatter = new Intl.NumberFormat("en-US", {
  notation: "compact",
  maximumFractionDigits: 1,
});

type VotingPowerPoint = {
  id: string;
  proposal_id: number | null;
  block_timestamp: string;
  timestamp_label: string;
  full_timestamp: string;
  effective_voting_power: number;
  effective_voting_power_label: string;
  delta_label: string;
  reason_label: string;
  event_type_label: string;
};

function toChartNumber(amount: VenearTokenAmountDto): number {
  const parsed = Number(amount.decimal);
  return Number.isFinite(parsed) ? parsed : 0;
}

function toPoints(
  events: VenearAccountVotingPowerEventDto[],
): VotingPowerPoint[] {
  return [...events]
    .sort(
      (left, right) =>
        new Date(left.refs.block_timestamp).getTime() -
        new Date(right.refs.block_timestamp).getTime(),
    )
    .map((event) => {
      const timestamp = new Date(event.refs.block_timestamp);

      return {
        id: `${event.refs.receipt_id}:${event.event_type}:${event.reason}`,
        proposal_id: event.proposal_id,
        block_timestamp: event.refs.block_timestamp,
        timestamp_label: axisDateFormatter.format(timestamp),
        full_timestamp: tooltipDateFormatter.format(timestamp),
        effective_voting_power: toChartNumber(
          event.effective_voting_power_after,
        ),
        effective_voting_power_label: formatTokenAmount(
          event.effective_voting_power_after,
        ),
        delta_label: formatTokenAmount(event.delta),
        reason_label: humanizeSnakeCase(event.reason),
        event_type_label: humanizeSnakeCase(event.event_type),
      };
    });
}

export function AccountVotingPowerChart({
  events,
}: {
  events: VenearAccountVotingPowerEventDto[];
}) {
  const points = toPoints(events);
  const latestPoint = points.at(-1);
  const earliestPoint = points[0];
  const tablePoints = [...points].reverse();

  return (
    <div className="space-y-6">
      <div className="grid gap-3 md:grid-cols-3">
        <div className="rounded-lg border bg-muted/30 p-4">
          <div className="mb-2 flex items-center gap-2 text-sm font-medium text-foreground">
            <TrendingUp className="size-4 text-primary" />
            Current effective power
          </div>
          <p className="text-2xl font-semibold tracking-[-0.04em] text-foreground">
            {latestPoint?.effective_voting_power_label ?? "—"}
          </p>
          <p className="mt-2 text-sm text-muted-foreground">
            Latest mutation in the current chart window.
          </p>
        </div>
        <div className="rounded-lg border bg-muted/30 p-4">
          <div className="mb-2 flex items-center gap-2 text-sm font-medium text-foreground">
            <Activity className="size-4 text-primary" />
            Mutations plotted
          </div>
          <p className="text-2xl font-semibold tracking-[-0.04em] text-foreground">
            {points.length}
          </p>
          <p className="mt-2 text-sm text-muted-foreground">
            The chart uses the latest voting-power mutation events returned by the API.
          </p>
        </div>
        <div className="rounded-lg border bg-muted/30 p-4">
          <div className="mb-2 flex items-center gap-2 text-sm font-medium text-foreground">
            <ArrowUpRight className="size-4 text-primary" />
            Span covered
          </div>
          <p className="text-lg font-semibold text-foreground">
            {earliestPoint?.timestamp_label ?? "—"} to{" "}
            {latestPoint?.timestamp_label ?? "—"}
          </p>
          <p className="mt-2 text-sm text-muted-foreground">
            Ordered chronologically so the curve reflects how the effective balance changed.
          </p>
        </div>
      </div>

      <ChartContainer
        config={chartConfig}
        className="h-[320px] w-full rounded-lg border bg-muted/20 p-4"
      >
        <AreaChart
          accessibilityLayer
          data={points}
          margin={{ left: 8, right: 8, top: 12, bottom: 0 }}
        >
          <defs>
            <linearGradient id="fill-effective-voting-power" x1="0" y1="0" x2="0" y2="1">
              <stop
                offset="5%"
                stopColor="var(--color-effective_voting_power)"
                stopOpacity={0.28}
              />
              <stop
                offset="95%"
                stopColor="var(--color-effective_voting_power)"
                stopOpacity={0.03}
              />
            </linearGradient>
          </defs>
          <CartesianGrid vertical={false} />
          <XAxis
            dataKey="timestamp_label"
            tickLine={false}
            axisLine={false}
            minTickGap={28}
          />
          <YAxis
            tickLine={false}
            axisLine={false}
            width={56}
            tickFormatter={(value) =>
              compactNumberFormatter.format(Number(value))
            }
          />
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                indicator="line"
                labelFormatter={(_, payload) =>
                  payload?.[0]?.payload?.full_timestamp ?? ""
                }
                formatter={(_, __, item) => {
                  const point = item.payload as VotingPowerPoint;

                  return (
                    <div className="space-y-1">
                      <p className="font-medium text-foreground">
                        {point.effective_voting_power_label}
                      </p>
                      <p className="text-muted-foreground">
                        {point.reason_label} • {point.event_type_label}
                      </p>
                      <p className="text-muted-foreground">
                        Delta {point.delta_label}
                      </p>
                    </div>
                  );
                }}
              />
            }
          />
          <Area
            type="monotone"
            dataKey="effective_voting_power"
            stroke="var(--color-effective_voting_power)"
            fill="url(#fill-effective-voting-power)"
            fillOpacity={1}
            strokeWidth={2.5}
            activeDot={{
              r: 5,
            }}
          />
        </AreaChart>
      </ChartContainer>

      <div className="space-y-3">
        <div>
          <h3 className="text-sm font-medium text-foreground">All mutations</h3>
          <p className="text-sm text-muted-foreground">
            Full voting-power event history for the current account window, newest first.
          </p>
        </div>
        <Table className="min-w-[56rem]">
          <TableHeader>
            <TableRow>
              <TableHead>When</TableHead>
              <TableHead>Reason</TableHead>
              <TableHead>Event</TableHead>
              <TableHead>Proposal</TableHead>
              <TableHead>Delta</TableHead>
              <TableHead>Effective after</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {tablePoints.map((point) => (
              <TableRow key={point.id}>
                <TableCell>{formatTimestamp(point.block_timestamp)}</TableCell>
                <TableCell>{point.reason_label}</TableCell>
                <TableCell>{point.event_type_label}</TableCell>
                <TableCell>
                  {point.proposal_id !== null ? (
                    <Link
                      href={`/proposals/${point.proposal_id}`}
                      className="font-medium text-foreground hover:text-primary"
                    >
                      Proposal #{point.proposal_id}
                    </Link>
                  ) : (
                    "—"
                  )}
                </TableCell>
                <TableCell>{point.delta_label}</TableCell>
                <TableCell>{point.effective_voting_power_label}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
