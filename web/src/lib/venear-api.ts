import "server-only";

import { cache } from "react";

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type QueryScalar = string | number | boolean | null | undefined;
type QueryValue = QueryScalar | QueryScalar[];

const DEFAULT_REVALIDATE_SECONDS = 30;

export interface VenearPaginatedResponse<T> {
  data: T[];
  next_cursor: string | null;
  has_more: boolean;
}

export interface VenearTokenAmountDto {
  raw: string;
  decimal: string;
  symbol: string;
  decimals: number;
}

export interface VenearChainRefDto {
  contract_id: string;
  transaction_hash: string | null;
  receipt_id: string;
  execution_outcome_id: string;
  block_height: number;
  block_timestamp: string;
}

export interface VenearProposalSnapshotDto {
  block_height: number | null;
  root: string | null;
  total_venear: VenearTokenAmountDto;
}

export interface VenearProposalTallyDto {
  option_index: number;
  option_label: string | null;
  votes_count: number;
  voting_power: VenearTokenAmountDto;
}

export interface VenearProposalSummaryDto {
  proposal_id: number;
  contract_id: string;
  title: string | null;
  description: string | null;
  link: string | null;
  voting_options: string[];
  status: string;
  proposer_id: string;
  reviewer_id: string | null;
  created_at: string;
  approved_at: string | null;
  rejected_at: string | null;
  voting_start_at: string | null;
  voting_end_at: string | null;
  snapshot: VenearProposalSnapshotDto | null;
  tally: VenearProposalTallyDto[];
  turnout_voter_count: number;
  turnout_voting_power: VenearTokenAmountDto;
  latest_event_type: string;
  refs: VenearChainRefDto;
}

export interface VenearProposalMilestonesDto {
  created_at: string;
  approved_at: string | null;
  rejected_at: string | null;
  voting_start_at: string | null;
  voting_end_at: string | null;
}

export interface VenearProposalDetailDto extends VenearProposalSummaryDto {
  metadata: JsonValue;
  milestones: VenearProposalMilestonesDto;
}

export interface VenearProposalTimelineEventDto {
  event_type: string;
  proposal_id: number;
  actor_account_id: string | null;
  status_after_event: string;
  refs: VenearChainRefDto;
  payload: JsonValue;
}

export interface VenearProposalVoteDto {
  proposal_id: number;
  voter_account_id: string;
  vote_option: number;
  vote_label: string | null;
  voting_power: VenearTokenAmountDto;
  voted_at: string;
  refs: VenearChainRefDto;
}

export interface VenearProposalVoteHistoryEventDto {
  proposal_id: number;
  voter_account_id: string;
  event_type: string;
  vote_option: number | null;
  vote_label: string | null;
  voting_power: VenearTokenAmountDto;
  effective_after_event: boolean;
  refs: VenearChainRefDto;
}

export interface VenearAccountSummaryDto {
  account_id: string;
  roles: string[];
  effective_voting_power: VenearTokenAmountDto;
  own_voting_power: VenearTokenAmountDto;
  delegated_voting_power: VenearTokenAmountDto;
  delegated_to_account_id: string | null;
  inbound_delegator_count: number;
  has_lockup: boolean;
  latest_activity_at: string | null;
}

export interface VenearAccountDelegationSummaryDto {
  delegated_to_account_id: string | null;
  inbound_delegator_count: number;
  has_inbound_delegation: boolean;
  has_outbound_delegation: boolean;
}

export interface VenearAccountLockupSummaryDto {
  lockup_account_id: string;
  status: string;
  locked_near_balance: VenearTokenAmountDto;
  last_updated_at: string;
}

export interface VenearAccountParticipationDto {
  proposals_created_count: number;
  proposals_reviewed_count: number;
  vote_events_count: number;
  current_effective_votes_count: number;
}

export interface VenearAccountGovernanceStateDto {
  current_effective_votes_count: number;
  proposals_created_count: number;
  proposals_reviewed_count: number;
}

export interface VenearAccountDetailDto {
  account_id: string;
  roles: string[];
  effective_voting_power: VenearTokenAmountDto;
  own_voting_power: VenearTokenAmountDto;
  delegated_voting_power: VenearTokenAmountDto;
  delegation: VenearAccountDelegationSummaryDto;
  lockup: VenearAccountLockupSummaryDto | null;
  participation: VenearAccountParticipationDto;
  latest_activity_at: string | null;
  latest_refs: VenearChainRefDto | null;
  governance_state: VenearAccountGovernanceStateDto | null;
}

export interface VenearAccountVoteDto {
  proposal_id: number;
  proposal_title: string | null;
  vote_option: number;
  vote_label: string | null;
  voting_power: VenearTokenAmountDto;
  voted_at: string;
  refs: VenearChainRefDto;
}

export interface VenearAccountVotingPowerEventDto {
  account_id: string;
  reason: string;
  event_type: string;
  proposal_id: number | null;
  delta: VenearTokenAmountDto;
  effective_voting_power_after: VenearTokenAmountDto;
  refs: VenearChainRefDto;
}

export interface VenearAccountDelegationHistoryEventDto {
  account_id: string;
  direction: string;
  event_type: string;
  counterparty_account_id: string | null;
  delegated_voting_power: VenearTokenAmountDto;
  refs: VenearChainRefDto;
}

export interface VenearDelegationEdgeDto {
  delegator_account_id: string;
  delegatee_account_id: string;
  last_observed_delegated_voting_power: VenearTokenAmountDto;
  started_at: string;
  last_changed_at: string;
  refs: VenearChainRefDto;
}

export interface VenearLockupSummaryDto {
  lockup_account_id: string;
  owner_account_id: string;
  contract_id: string;
  status: string;
  locked_near_balance: VenearTokenAmountDto;
  delegated_to_account_id: string | null;
  last_updated_at: string;
  refs: VenearChainRefDto;
}

export interface VenearLockupDetailDto {
  lockup_account_id: string;
  owner_account_id: string;
  contract_id: string;
  status: string;
  locked_near_balance: VenearTokenAmountDto;
  delegated_to_account_id: string | null;
  deployment_refs: VenearChainRefDto | null;
  latest_refs: VenearChainRefDto;
}

export interface VenearLockupEventDto {
  lockup_account_id: string;
  owner_account_id: string;
  event_type: string;
  locked_near_balance: VenearTokenAmountDto | null;
  refs: VenearChainRefDto;
  payload: JsonValue;
}

export interface VenearRewardEventDto {
  account_id: string;
  event_type: string;
  amount: VenearTokenAmountDto | null;
  refs: VenearChainRefDto;
  payload: JsonValue;
}

export interface VenearConfigSnapshotDto {
  contract_id: string;
  owner_account_id: string | null;
  proposed_new_owner_account_id: string | null;
  guardians: string[];
  reviewer_ids: string[];
  lockup_contract_hash: string | null;
  lockup_code_deployers: string[];
  unlock_duration_sec: number | null;
  min_lockup_deposit: VenearTokenAmountDto | null;
  local_deposit: VenearTokenAmountDto | null;
  staking_pool_whitelist_account_id: string | null;
  paused: boolean | null;
  refs: VenearChainRefDto | null;
  extra_fields: JsonValue;
}

export interface VenearAdminActionDto {
  contract_id: string;
  event_type: string;
  actor_account_id: string | null;
  success: boolean;
  refs: VenearChainRefDto;
  payload: JsonValue;
}

export interface VenearActivityItemDto {
  kind: string;
  event_type: string;
  actor_account_id: string | null;
  subject_account_id: string | null;
  proposal_id: number | null;
  success: boolean;
  refs: VenearChainRefDto;
  payload: JsonValue;
}

interface ApiProblemDetails {
  title?: string;
  detail?: string;
}

export class VenearApiError extends Error {
  readonly status: number;
  readonly url: string;

  constructor(status: number, message: string, url: string) {
    super(message);
    this.name = "VenearApiError";
    this.status = status;
    this.url = url;
  }
}

function resolveApiBaseUrl(): string {
  const configured =
    process.env.VENEAR_API_BASE_URL ??
    process.env.NEXT_PUBLIC_VENEAR_API_BASE_URL ??
    "http://127.0.0.1:3000/api/v1/venear";

  return configured.endsWith("/") ? configured : `${configured}/`;
}

function buildApiUrl(pathname: string, query?: Record<string, QueryValue>): string {
  const url = new URL(pathname.replace(/^\//, ""), resolveApiBaseUrl());

  for (const [key, value] of Object.entries(query ?? {})) {
    appendQueryValue(url.searchParams, key, value);
  }

  return url.toString();
}

function appendQueryValue(
  searchParams: URLSearchParams,
  key: string,
  value: QueryValue,
): void {
  if (Array.isArray(value)) {
    for (const item of value) {
      appendQueryValue(searchParams, key, item);
    }
    return;
  }

  if (value === null || value === undefined || value === "") {
    return;
  }

  searchParams.set(key, String(value));
}

async function fetchJson<T>(
  pathname: string,
  query?: Record<string, QueryValue>,
): Promise<T> {
  const url = buildApiUrl(pathname, query);
  const response = await fetch(url, {
    headers: {
      accept: "application/json",
    },
    next: {
      revalidate: DEFAULT_REVALIDATE_SECONDS,
    },
  });

  if (!response.ok) {
    let message = `Request failed with status ${response.status}`;

    try {
      const problem = (await response.json()) as ApiProblemDetails;
      message = problem.detail ?? problem.title ?? message;
    } catch {
      // Fall back to the default message when the response is not JSON.
    }

    throw new VenearApiError(response.status, message, url);
  }

  return (await response.json()) as T;
}

export function isVenearApiError(error: unknown): error is VenearApiError {
  return error instanceof VenearApiError;
}

export async function listProposals(
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearProposalSummaryDto>> {
  return fetchJson("proposals", query);
}

export const getProposal = cache(async (proposalId: number) => {
  return fetchJson<VenearProposalDetailDto>(`proposals/${proposalId}`);
});

export async function listProposalTimeline(
  proposalId: number,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearProposalTimelineEventDto>> {
  return fetchJson(`proposals/${proposalId}/timeline`, query);
}

export async function listProposalVotes(
  proposalId: number,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearProposalVoteDto>> {
  return fetchJson(`proposals/${proposalId}/votes`, query);
}

export async function listProposalVoteHistory(
  proposalId: number,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearProposalVoteHistoryEventDto>> {
  return fetchJson(`proposals/${proposalId}/vote_history`, query);
}

export async function listAccounts(
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearAccountSummaryDto>> {
  return fetchJson("accounts", query);
}

export const getAccount = cache(async (accountId: string) => {
  return fetchJson<VenearAccountDetailDto>(
    `accounts/${encodeURIComponent(accountId)}`,
  );
});

export async function listAccountProposals(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearProposalSummaryDto>> {
  return fetchJson(`accounts/${encodeURIComponent(accountId)}/proposals`, query);
}

export async function listAccountVotes(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearAccountVoteDto>> {
  return fetchJson(`accounts/${encodeURIComponent(accountId)}/votes`, query);
}

export async function listAccountVoteHistory(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearProposalVoteHistoryEventDto>> {
  return fetchJson(
    `accounts/${encodeURIComponent(accountId)}/vote_history`,
    query,
  );
}

export async function listAccountVotingPowerHistory(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearAccountVotingPowerEventDto>> {
  return fetchJson(
    `accounts/${encodeURIComponent(accountId)}/voting_power_history`,
    query,
  );
}

export async function listAccountDelegationHistory(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearAccountDelegationHistoryEventDto>> {
  return fetchJson(
    `accounts/${encodeURIComponent(accountId)}/delegation_history`,
    query,
  );
}

export async function listAccountLockupEvents(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearLockupEventDto>> {
  return fetchJson(
    `accounts/${encodeURIComponent(accountId)}/lockup_events`,
    query,
  );
}

export async function listAccountRewardsEvents(
  accountId: string,
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearRewardEventDto>> {
  return fetchJson(
    `accounts/${encodeURIComponent(accountId)}/rewards_events`,
    query,
  );
}

export async function listDelegations(
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearDelegationEdgeDto>> {
  return fetchJson("delegations", query);
}

export async function listLockups(
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearLockupSummaryDto>> {
  return fetchJson("lockups", query);
}

export async function listConfig(): Promise<{ data: VenearConfigSnapshotDto[] }> {
  return fetchJson("config");
}

export async function listAdminActions(
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearAdminActionDto>> {
  return fetchJson("admin/actions", query);
}

export async function listActivity(
  query?: Record<string, QueryValue>,
): Promise<VenearPaginatedResponse<VenearActivityItemDto>> {
  return fetchJson("activity", query);
}
