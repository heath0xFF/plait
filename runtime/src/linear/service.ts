import { createHash } from 'node:crypto';
import { fail, HostError } from '../protocol.ts';
import { checkout, rootIdentity } from '../safety.ts';
import type { Store } from '../store.ts';
import { LinearAuthStore, formatAuthStatus, validateLinearKey } from './auth.ts';
import type { StoredLinearAuth } from './auth.ts';
import { LinearClient } from './client.ts';
import { LINEAR_WRITE_HISTORY_LIMIT } from './writes.ts';
import type { WriteIntent, WriteReceipt } from './writes.ts';
import type { LinearClientOptions } from './client.ts';
import {
  LINEAR_LINKS_DEFAULT_LIMIT,
  asFilterPreset,
  handlesLinearOp,
  parseLinkCursor,
  validateLinearRequest,
} from './protocol.ts';
import {
  FIELD_LIMITS,
  LINEAR_CACHE_DETAIL_TTL_MS,
  LINEAR_CACHE_ISSUES_TTL_MS,
  LINEAR_CACHE_TEAMS_TTL_MS,
  LinearError,
  boundLinearText,
  safeLinearUrl,
} from './types.ts';
import type {
  LinearAuthStatus,
  LinearFilterPreset,
  LinearIssueDetail,
  LinearIssueSummary,
  LinearProjectBinding,
  LinearTeam,
  LinearThreadLink,
} from './types.ts';

export type LinearServiceOptions = LinearClientOptions & {
  now?: () => number;
};

const OFFLINE_TEAMS_WARNING = 'Linear could not be reached. Showing cached teams.';
const OFFLINE_ISSUES_WARNING = 'Linear could not be reached. Showing cached issues.';
const OFFLINE_ISSUE_WARNING = 'Linear could not be reached. Showing cached issue.';

function cacheAgeSeconds(cachedAt: number, now: number): number {
  return Math.max(0, Math.floor((now - cachedAt) / 1000));
}

function pageCacheKey(teamId: string, filter: LinearFilterPreset, search: string, cursor: string): string {
  return JSON.stringify([teamId, filter, search, cursor]);
}

/**
 * Host-side Linear integration: API-key auth, bounded GraphQL reads, and SQLite cache.
 * Performs zero network work unless explicitly called by a host socket operation.
 */
export class LinearService {
  private readonly store: Store;
  private readonly authStore: LinearAuthStore;
  private readonly client: LinearClient;
  private readonly now: () => number;
  private keyRejected = false;
  /** Increments on sign-in, sign-out, and cache invalidations to fence stale in-flight replies. */
  private authGeneration = 0;
  private cacheGeneration = 0;
  private setKeySequence = 0;
  private authMutation: Promise<void> = Promise.resolve();
  private readonly querySequence = new Map<string, number>();
  private readonly inFlightWrites = new Map<string, { authGen: number; promise: Promise<unknown> }>();

  constructor(store: Store, storeDirectory: string, options: LinearServiceOptions = {}) {
    this.store = store;
    this.authStore = new LinearAuthStore(storeDirectory);
    this.now = options.now ?? Date.now;
    this.client = new LinearClient({ ...options, now: this.now });
  }

  handles(input: unknown): boolean {
    return handlesLinearOp(input);
  }

  async handle(input: unknown): Promise<unknown> {
    const request = validateLinearRequest(input);
    switch (request.op) {
      case 'linear_info': return { schema: 1, features: { durable_write_outcomes: true }, write_history_limit: LINEAR_WRITE_HISTORY_LIMIT, write_history_retention: 'store_lifetime' };
      case 'linear_get_write': return this.getWriteOutcome(String(request.request_id));
      case 'linear_auth_status': return this.authStatus();
      case 'linear_set_key': return this.setKey(validateLinearKey(request.key));
      case 'linear_sign_out': return this.signOut();
      case 'linear_list_teams': return this.listTeams(request.refresh === true);
      case 'linear_get_project_binding': return this.getProjectBinding(String(request.checkout));
      case 'linear_bind_project': return this.bindProject(
        String(request.checkout),
        request.team_id === null ? null : String(request.team_id),
        typeof request.team_key === 'string' ? request.team_key : null,
        typeof request.team_name === 'string' ? request.team_name.trim() : null,
      );
      case 'linear_list_issues': return this.listIssues({
        teamId: String(request.team_id),
        filter: asFilterPreset(request.filter),
        search: typeof request.search === 'string' ? request.search.trim() : '',
        cursor: typeof request.cursor === 'string' ? request.cursor : undefined,
        refresh: request.refresh === true,
      });
      case 'linear_get_issue': return this.getIssue({
        issueId: String(request.issue_id),
        commentsCursor: typeof request.comments_cursor === 'string' ? request.comments_cursor : undefined,
        refresh: request.refresh === true,
      });
      case 'linear_link_thread': return this.linkThread({
        threadId: String(request.thread_id),
        issueId: String(request.issue_id),
        identifier: String(request.identifier),
        url: String(request.url),
        title: String(request.title),
      });
      case 'linear_unlink_thread': return this.unlinkThread(String(request.thread_id));
      case 'linear_get_thread_link': return this.getThreadLink(String(request.thread_id));
      case 'linear_list_thread_links': return this.listThreadLinks(
        typeof request.limit === 'number' ? request.limit : LINEAR_LINKS_DEFAULT_LIMIT,
        typeof request.cursor === 'string' ? request.cursor : undefined,
      );
      case 'linear_post_comment': return this.withWriteOutcome(String(request.request_id), () => this.postComment({
        issueId: String(request.issue_id),
        body: String(request.body),
        requestId: String(request.request_id),
      }));
      case 'linear_update_issue_state': return this.withWriteOutcome(String(request.request_id), () => this.updateIssueState({
        issueId: String(request.issue_id),
        stateId: String(request.state_id),
        requestId: String(request.request_id),
      }));
      default: fail('invalid_request', 'Unknown operation or field.');
    }
  }

  async authStatus(): Promise<LinearAuthStatus> {
    const auth = await this.authStore.read();
    return formatAuthStatus(auth, this.keyRejected);
  }

  private serializeAuthMutation<T>(work: () => Promise<T>): Promise<T> {
    const next = this.authMutation.then(work, work);
    this.authMutation = next.then(() => {}, () => {});
    return next;
  }

  async setKey(apiKey: string): Promise<LinearAuthStatus> {
    const authGen = this.authGeneration;
    const seq = ++this.setKeySequence;
    const verified = await this.client.viewer(apiKey);
    return this.serializeAuthMutation(async () => {
      if (this.authGeneration !== authGen || this.setKeySequence !== seq) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
      const previous = await this.authStore.read().catch(() => null);
      const saved: StoredLinearAuth = {
        ...verified.data,
        api_key: apiKey,
        saved_at: this.now(),
      };
      await this.authStore.write(saved);
      this.authGeneration++;
      this.cacheGeneration++;
      this.keyRejected = false;
      this.inFlightWrites.clear();
      this.store.linear.clearAll({ clearProjects: previous !== null && previous.workspace_id !== saved.workspace_id });
      return formatAuthStatus(saved, false);
    });
  }

  async signOut(): Promise<LinearAuthStatus> {
    // Advance generations first so any in-flight read or setKey cannot repopulate state after sign-out.
    this.authGeneration++;
    this.setKeySequence++;
    this.cacheGeneration++;
    this.keyRejected = false;
    this.inFlightWrites.clear();
    return this.serializeAuthMutation(async () => {
      await this.authStore.remove();
      this.store.linear.clearAll({ clearProjects: true });
      return formatAuthStatus(null, false);
    });
  }

  private async requireAuth(): Promise<{ auth: StoredLinearAuth; authGen: number; cacheGen: number }> {
    const authGen = this.authGeneration;
    const auth = await this.authStore.read();
    if (this.authGeneration !== authGen) throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
    if (!auth) throw new LinearError('not_signed_in', 'Linear is not signed in. Set an API key in Settings.');
    return { auth, authGen: this.authGeneration, cacheGen: this.cacheGeneration };
  }

  private nextQuery(scope: string): number {
    const next = (this.querySequence.get(scope) ?? 0) + 1;
    this.querySequence.set(scope, next);
    return next;
  }

  private canPopulateCache(scope: string, seq: number, authGen: number, cacheGen: number): boolean {
    return this.authGeneration === authGen
      && this.cacheGeneration === cacheGen
      && this.querySequence.get(scope) === seq;
  }

  private noteError(error: unknown, authGen: number): never {
    if (error instanceof LinearError && error.kind === 'key_rejected' && this.authGeneration === authGen) {
      this.keyRejected = true;
    }
    throw error;
  }

  async listTeams(refresh = false): Promise<{
    teams: LinearTeam[];
    cached: boolean;
    stale: boolean;
    cached_at: number;
    cache_age_seconds: number;
    partial: boolean;
    warning: string | null;
  }> {
    const { auth, authGen, cacheGen } = await this.requireAuth();
    const now = this.now();
    const cached = this.store.linear.getTeams();
    if (!refresh && cached && now - cached.cached_at < LINEAR_CACHE_TEAMS_TTL_MS) {
      return {
        teams: cached.teams,
        cached: true,
        stale: false,
        cached_at: cached.cached_at,
        cache_age_seconds: cacheAgeSeconds(cached.cached_at, now),
        partial: false,
        warning: null,
      };
    }
    const scope = 'teams';
    const seq = this.nextQuery(scope);
    try {
      const fresh = await this.client.listTeams(auth.api_key);
      if (this.authGeneration !== authGen) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
      this.keyRejected = false;
      const fetchedAt = this.now();
      if (!fresh.partial && this.canPopulateCache(scope, seq, authGen, cacheGen)) {
        this.store.linear.saveTeams(fresh.data, fetchedAt);
      }
      return {
        teams: fresh.data,
        cached: false,
        stale: false,
        cached_at: fetchedAt,
        cache_age_seconds: 0,
        partial: fresh.partial,
        warning: fresh.warning,
      };
    } catch (error) {
      if (error instanceof LinearError && error.kind === 'offline' && cached && this.authGeneration === authGen) {
        const at = this.now();
        return {
          teams: cached.teams,
          cached: true,
          stale: true,
          cached_at: cached.cached_at,
          cache_age_seconds: cacheAgeSeconds(cached.cached_at, at),
          partial: false,
          warning: OFFLINE_TEAMS_WARNING,
        };
      }
      this.noteError(error, authGen);
    }
  }

  async getProjectBinding(rawCheckout: string): Promise<{ checkout: string; binding: LinearProjectBinding | null }> {
    const root = await checkout(rawCheckout);
    const identity = await rootIdentity(root);
    const auth = await this.authStore.read();
    if (!auth) return { checkout: root, binding: null };
    const binding = this.store.linear.getProjectBinding(root, identity);
    if (binding && binding.workspace_id !== null && binding.workspace_id !== auth.workspace_id) {
      return { checkout: root, binding: null };
    }
    return { checkout: root, binding };
  }

  async bindProject(
    rawCheckout: string,
    teamId: string | null,
    explicitKey: string | null,
    explicitName: string | null,
  ): Promise<{ checkout: string; binding: LinearProjectBinding | null }> {
    const root = await checkout(rawCheckout);
    const identity = await rootIdentity(root);
    if (teamId === null) {
      this.store.linear.setProjectBinding(root, identity, null);
      return { checkout: root, binding: null };
    }
    const auth = await this.authStore.read();
    const team = this.store.linear.getTeam(teamId);
    const binding: LinearProjectBinding = {
      workspace_id: auth?.workspace_id ?? null,
      team_id: teamId,
      team_key: explicitKey ? boundLinearText(explicitKey, FIELD_LIMITS.key) : team?.key ?? null,
      team_name: explicitName ? boundLinearText(explicitName, FIELD_LIMITS.name) : team?.name ?? null,
      updated_at: this.now(),
    };
    this.store.linear.setProjectBinding(root, identity, binding);
    return { checkout: root, binding };
  }

  async listIssues(params: {
    teamId: string;
    filter: LinearFilterPreset;
    search: string;
    cursor?: string;
    refresh: boolean;
  }): Promise<{
    issues: LinearIssueSummary[];
    has_more: boolean;
    next_cursor: string | null;
    cached: boolean;
    stale: boolean;
    cached_at: number;
    cache_age_seconds: number;
    partial: boolean;
    warning: string | null;
  }> {
    const { auth, authGen, cacheGen } = await this.requireAuth();
    const key = pageCacheKey(params.teamId, params.filter, params.search, params.cursor ?? '');
    const now = this.now();
    const cached = this.store.linear.getIssuesPage(key, now);
    if (!params.refresh && cached && now - cached.cached_at < LINEAR_CACHE_ISSUES_TTL_MS) {
      return {
        issues: cached.issues,
        has_more: cached.has_more,
        next_cursor: cached.next_cursor,
        cached: true,
        stale: false,
        cached_at: cached.cached_at,
        cache_age_seconds: cacheAgeSeconds(cached.cached_at, now),
        partial: false,
        warning: null,
      };
    }
    const scope = `issues:${key}`;
    const seq = this.nextQuery(scope);
    const teamScope = `team-issues:${params.teamId}`;
    const teamSeq = this.nextQuery(teamScope);
    try {
      const fresh = await this.client.listIssues(auth.api_key, {
        teamId: params.teamId,
        filter: params.filter,
        search: params.search || undefined,
        cursor: params.cursor,
      });
      if (this.authGeneration !== authGen) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
      this.keyRejected = false;
      const fetchedAt = this.now();
      if (!fresh.partial && this.canPopulateCache(scope, seq, authGen, cacheGen) && this.querySequence.get(teamScope) === teamSeq) {
        this.store.linear.saveIssuesPage(
          key,
          params.teamId,
          fresh.data.issues,
          fresh.data.has_more,
          fresh.data.next_cursor,
          fetchedAt,
        );
      }
      return {
        issues: fresh.data.issues,
        has_more: fresh.data.has_more,
        next_cursor: fresh.data.next_cursor,
        cached: false,
        stale: false,
        cached_at: fetchedAt,
        cache_age_seconds: 0,
        partial: fresh.partial,
        warning: fresh.warning,
      };
    } catch (error) {
      if (error instanceof LinearError && error.kind === 'offline' && cached && this.authGeneration === authGen) {
        const at = this.now();
        return {
          issues: cached.issues,
          has_more: cached.has_more,
          next_cursor: cached.next_cursor,
          cached: true,
          stale: true,
          cached_at: cached.cached_at,
          cache_age_seconds: cacheAgeSeconds(cached.cached_at, at),
          partial: false,
          warning: OFFLINE_ISSUES_WARNING,
        };
      }
      this.noteError(error, authGen);
    }
  }

  async getIssue(params: {
    issueId: string;
    commentsCursor?: string;
    refresh: boolean;
  }): Promise<{
    issue: LinearIssueDetail;
    cached: boolean;
    stale: boolean;
    cached_at: number;
    cache_age_seconds: number;
    partial: boolean;
    warning: string | null;
  }> {
    const { auth, authGen, cacheGen } = await this.requireAuth();
    const now = this.now();
    const isFirstPage = params.commentsCursor === undefined;
    const cached = isFirstPage ? this.store.linear.getIssueDetail(params.issueId, now) : null;
    if (!params.refresh && cached && now - cached.cached_at < LINEAR_CACHE_DETAIL_TTL_MS) {
      return {
        issue: cached.issue,
        cached: true,
        stale: false,
        cached_at: cached.cached_at,
        cache_age_seconds: cacheAgeSeconds(cached.cached_at, now),
        partial: false,
        warning: null,
      };
    }
    const scope = `detail:${params.issueId}:${params.commentsCursor ?? ''}`;
    const seq = this.nextQuery(scope);
    try {
      const fresh = await this.client.getIssue(auth.api_key, {
        issueId: params.issueId,
        commentsCursor: params.commentsCursor,
      });
      if (this.authGeneration !== authGen) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
      this.keyRejected = false;
      if (!fresh.data) fail('not_found', 'Linear issue not found.');
      const fetchedAt = this.now();
      if (isFirstPage && !fresh.partial && this.canPopulateCache(scope, seq, authGen, cacheGen)) {
        if (fresh.data.team_id) this.nextQuery(`team-issues:${fresh.data.team_id}`);
        this.store.linear.saveIssueDetail(fresh.data, fetchedAt);
      }
      return {
        issue: fresh.data,
        cached: false,
        stale: false,
        cached_at: fetchedAt,
        cache_age_seconds: 0,
        partial: fresh.partial,
        warning: fresh.warning,
      };
    } catch (error) {
      if (error instanceof LinearError && error.kind === 'offline' && cached && this.authGeneration === authGen) {
        const at = this.now();
        return {
          issue: cached.issue,
          cached: true,
          stale: true,
          cached_at: cached.cached_at,
          cache_age_seconds: cacheAgeSeconds(cached.cached_at, at),
          partial: false,
          warning: OFFLINE_ISSUE_WARNING,
        };
      }
      this.noteError(error, authGen);
    }
  }

  /** Persists a local link between a thread and a Linear issue. Never writes to Linear. */
  linkThread(params: {
    threadId: string;
    issueId: string;
    identifier: string;
    url: string;
    title: string;
  }): { link: LinearThreadLink } {
    if (!this.store.linear.hasThread(params.threadId)) fail('not_found', 'Thread not found.');
    const safeUrl = safeLinearUrl(params.url);
    if (!safeUrl) fail('invalid_request', 'url must be a valid https URL.');
    const link = this.store.linear.setThreadLink({
      thread_id: params.threadId,
      issue_id: params.issueId,
      identifier: params.identifier,
      url: safeUrl,
      title: boundLinearText(params.title.trim(), FIELD_LIMITS.title),
      linked_at: this.now(),
    });
    return { link };
  }

  unlinkThread(threadId: string): { thread_id: string; removed: boolean } {
    if (!this.store.linear.hasThread(threadId)) fail('not_found', 'Thread not found.');
    return { thread_id: threadId, removed: this.store.linear.removeThreadLink(threadId) };
  }

  getThreadLink(threadId: string): { thread_id: string; link: LinearThreadLink | null } {
    if (!this.store.linear.hasThread(threadId)) fail('not_found', 'Thread not found.');
    return { thread_id: threadId, link: this.store.linear.getThreadLink(threadId) };
  }

  listThreadLinks(limit: number, cursor?: string) {
    return this.store.linear.listThreadLinks(limit, cursor ? parseLinkCursor(cursor) : undefined);
  }

  private async withWriteOutcome(requestId: string, work: () => Promise<WriteReceipt>): Promise<WriteReceipt> {
    try {
      return await work();
    } catch (error) {
      if (error instanceof HostError && !error.outcome) {
        error.outcome = this.store.linear.writes.get(requestId)?.outcome ?? 'not_sent';
      }
      throw error;
    }
  }

  getWriteOutcome(requestId: string) {
    const saved = this.store.linear.writes.get(requestId);
    if (!saved) return { request_id: requestId, write: null };
    const { hash: _hash, payload: _payload, ...write } = saved;
    // This is local history, including while signed out, never a live issue read.
    return { request_id: requestId, write };
  }

  async postComment(params: { issueId: string; body: string; requestId: string }): Promise<WriteReceipt> {
    return this.write(params.requestId, 'linear_post_comment', params.issueId, params.body,
      async () => {},
      async auth => this.client.createComment(auth.api_key, params.issueId, params.body,
        commentUuid(auth.workspace_id, params.requestId)));
  }

  async updateIssueState(params: { issueId: string; stateId: string; requestId: string }): Promise<WriteReceipt> {
    return this.write(params.requestId, 'linear_update_issue_state', params.issueId, params.stateId,
      async (auth, authGen) => {
        const teamId = await this.resolveIssueTeamId(params.issueId, auth.api_key, authGen);
        const team = await this.resolveTeamForValidation(teamId, params.stateId, authGen);
        if (!team || !team.states.some(state => state.id === params.stateId)) {
          fail('invalid_request', 'Workflow state does not belong to this issue\'s team.');
        }
      },
      async auth => {
        await this.client.updateIssueState(auth.api_key, params.issueId, params.stateId);
        return {};
      });
  }

  private async write(
    requestId: string, op: string, issueId: string, payload: string,
    validate: (auth: StoredLinearAuth, authGen: number) => Promise<void>,
    send: (auth: StoredLinearAuth) => Promise<{ comment_id?: string }>,
  ): Promise<WriteReceipt> {
    const { auth, authGen } = await this.requireAuth();
    // Never persist a pasted credential as an exact mutation payload or identifier.
    if ([requestId, issueId, payload].some(value => value.includes(auth.api_key) || value.includes(encodeURIComponent(auth.api_key)))) {
      fail('invalid_request', 'Linear write input must not contain the API key.');
    }
    const hash = writeDigest([issueId, payload]);
    const intent = this.store.linear.writes.begin({
      request_id: requestId, op, hash, workspace_id: auth.workspace_id,
      issue_id: issueId, payload, created_at: this.now(),
    });
    const inflight = this.inFlightWrites.get(requestId);
    if (inflight?.authGen === authGen) return inflight.promise as Promise<WriteReceipt>;
    if (intent.outcome === 'accepted') return this.acceptedReceipt(intent, authGen);
    if (intent.outcome === 'unknown') this.unknownWrite();
    const promise = this.executeWrite(intent, payload, auth, authGen, validate, send);
    this.inFlightWrites.set(requestId, { authGen, promise });
    try {
      return await promise;
    } finally {
      if (this.inFlightWrites.get(requestId)?.promise === promise) this.inFlightWrites.delete(requestId);
    }
  }

  private unknownWrite(): never {
    const error = new HostError('linear_write_unknown', 'Linear write is unconfirmed. Check its result in Linear; this request will not be sent again.');
    error.outcome = 'unknown';
    throw error;
  }

  private async executeWrite(
    intent: WriteIntent, payload: string, auth: StoredLinearAuth, authGen: number,
    validate: (auth: StoredLinearAuth, authGen: number) => Promise<void>,
    send: (auth: StoredLinearAuth) => Promise<{ comment_id?: string }>,
  ): Promise<WriteReceipt> {
    try {
      await validate(auth, authGen);
      if (this.authGeneration !== authGen) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
    } catch (error) {
      if (error instanceof HostError) error.outcome = intent.outcome;
      this.noteError(error, authGen);
    }
    if (!this.store.linear.writes.claim(intent.request_id, payload, this.now())) this.unknownWrite();
    let result: { comment_id?: string };
    try {
      result = await send(auth);
    } catch (error) {
      if (error instanceof LinearError && error.definitelyRejected) {
        this.store.linear.writes.finish(intent.request_id, 'rejected', null, this.now());
        error.outcome = 'rejected';
        this.noteError(error, authGen);
      }
      this.unknownWrite();
    }
    const receipt: WriteReceipt = { ...result, issue: null, partial: false, warning: null, outcome: 'accepted' };
    // Receipt durability is independent of the credential epoch and of follow-up reads.
    this.store.linear.writes.finish(intent.request_id, 'accepted', receipt, this.now());
    if (this.authGeneration === authGen) {
      this.cacheGeneration++;
      this.store.linear.invalidateIssue(intent.issue_id!);
    }
    const reply: WriteReceipt = { ...receipt, ...await this.refreshAfterWrite(intent.issue_id!, authGen) };
    if (this.authGeneration === authGen) {
      this.store.linear.saveWrite(intent.request_id, intent.op, intent.hash, reply, this.now());
    }
    return reply;
  }

  private async acceptedReceipt(intent: WriteIntent, authGen: number): Promise<WriteReceipt> {
    const full = this.store.linear.getWrite(intent.request_id)?.result as WriteReceipt | undefined;
    const receipt = { ...(full ?? intent.receipt!), outcome: 'accepted' as const };
    // Migrated receipts have no workspace identity; only their compact historical receipt is safe.
    if (!intent.workspace_id || !intent.issue_id) return intent.receipt!;
    if (receipt.issue) return receipt;
    const reply = { ...receipt, ...await this.refreshAfterWrite(intent.issue_id, authGen) };
    if (this.authGeneration === authGen) {
      this.store.linear.saveWrite(intent.request_id, intent.op, intent.hash, reply, this.now());
    }
    return reply;
  }

  private async resolveIssueTeamId(issueId: string, apiKey: string, authGen: number): Promise<string> {
    const cached = this.store.linear.findIssueTeamId(issueId);
    if (cached) return cached;
    try {
      const fetched = await this.client.getIssue(apiKey, { issueId });
      if (this.authGeneration !== authGen) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
      if (!fetched.data) fail('not_found', 'Linear issue not found.');
      if (!fetched.data.team_id) fail('invalid_request', 'Issue team could not be determined.');
      return fetched.data.team_id;
    } catch (error) {
      this.noteError(error, authGen);
    }
  }

  private async resolveTeamForValidation(teamId: string, stateId: string, authGen: number): Promise<LinearTeam | null> {
    const cached = this.store.linear.getTeams();
    const fresh = cached !== null && this.now() - cached.cached_at < LINEAR_CACHE_TEAMS_TTL_MS;
    const cachedTeam = fresh ? cached.teams.find(team => team.id === teamId) ?? null : null;
    if (cachedTeam && cachedTeam.states.some(state => state.id === stateId)) {
      return cachedTeam;
    }
    // Refresh from Linear when teams are missing, expired past TTL, or do not yet list the target state.
    const refreshed = await this.listTeams(true);
    if (this.authGeneration !== authGen) {
      throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
    }
    return refreshed.teams.find(team => team.id === teamId) ?? null;
  }

  private async refreshAfterWrite(
    issueId: string,
    authGen: number,
  ): Promise<{ issue: LinearIssueDetail | null; partial: boolean; warning: string | null }> {
    if (this.authGeneration !== authGen) {
      return { issue: null, partial: false, warning: 'Linear sign-in changed after the write succeeded.' };
    }
    try {
      const refreshed = await this.getIssue({ issueId, refresh: true });
      return { issue: refreshed.issue, partial: refreshed.partial, warning: refreshed.warning };
    } catch {
      return { issue: null, partial: false, warning: 'Write succeeded, but the refreshed issue could not be loaded.' };
    }
  }

}

function writeDigest(parts: unknown[]): string {
  return createHash('sha256').update(JSON.stringify(parts)).digest('hex');
}

/** Linear documents UUID v4 syntax; fix its version/variant bits on a domain-separated digest. */
function commentUuid(workspaceId: string, requestId: string): string {
  const bytes = createHash('sha256').update(JSON.stringify(['plait-linear-comment', workspaceId, requestId])).digest().subarray(0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString('hex');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
