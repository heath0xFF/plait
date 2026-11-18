import { createHash } from 'node:crypto';
import { fail } from '../protocol.ts';
import { checkout, rootIdentity } from '../safety.ts';
import type { Store } from '../store.ts';
import { LinearAuthStore, formatAuthStatus, validateLinearKey } from './auth.ts';
import type { StoredLinearAuth } from './auth.ts';
import { LinearClient } from './client.ts';
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
  private readonly inFlightWrites = new Map<string, { op: string; hash: string; authGen: number; promise: Promise<unknown> }>();

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
      case 'linear_auth_status': return this.authStatus();
      case 'linear_set_key': return this.setKey(validateLinearKey(request.key));
      case 'linear_sign_out': return this.signOut();
      case 'linear_list_teams': return this.listTeams(request.refresh === true);
      case 'linear_get_project_binding': return this.getProjectBinding(String(request.checkout));
      case 'linear_bind_project': return this.bindProject(
        String(request.checkout),
        request.team_id === null ? null : String(request.team_id),
        typeof request.team_key === 'string' ? request.team_key : null,
        typeof request.team_name === 'string' ? request.team_name : null,
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
      case 'linear_post_comment': return this.postComment({
        issueId: String(request.issue_id),
        body: String(request.body),
        requestId: String(request.request_id),
      });
      case 'linear_update_issue_state': return this.updateIssueState({
        issueId: String(request.issue_id),
        stateId: String(request.state_id),
        requestId: String(request.request_id),
      });
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
    const auth = await this.authStore.read();
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

  /**
   * Posts one comment on a Linear issue on an explicit person action.
   * Deduplicates by request_id so a retry after a lost reply never double-posts.
   */
  async postComment(params: {
    issueId: string;
    body: string;
    requestId: string;
  }): Promise<{ comment_id: string; issue: LinearIssueDetail | null; partial: boolean; warning: string | null }> {
    const op = 'linear_post_comment';
    const hash = writeDigest([params.issueId, params.body]);
    return this.deduplicatedWrite(params.requestId, op, hash, async () => {
      const { auth, authGen } = await this.requireAuth();
      let commentId: string;
      try {
        ({ comment_id: commentId } = await this.client.createComment(auth.api_key, params.issueId, params.body));
      } catch (error) {
        this.noteError(error, authGen);
      }
      // Record acceptance before refresh so a retry after a failed refresh never posts twice.
      const provisional = { comment_id: commentId, issue: null as LinearIssueDetail | null, partial: false, warning: null as string | null };
      if (this.authGeneration === authGen) {
        this.cacheGeneration++;
        this.store.linear.invalidateIssue(params.issueId);
        this.store.linear.saveWrite(params.requestId, op, hash, provisional, this.now());
      }
      const refreshed = await this.refreshAfterWrite(params.issueId, authGen);
      const reply = { comment_id: commentId, ...refreshed };
      if (this.authGeneration === authGen) {
        this.store.linear.saveWrite(params.requestId, op, hash, reply, this.now());
      }
      return reply;
    }, async existing => {
      const prior = existing as { comment_id: string; issue: LinearIssueDetail | null; partial: boolean; warning: string | null };
      if (prior.issue !== null) return prior;
      // The comment already succeeded earlier; retry only the read refresh.
      const { authGen } = await this.requireAuth();
      const refreshed = await this.refreshAfterWrite(params.issueId, authGen);
      const reply = { comment_id: prior.comment_id, ...refreshed };
      if (this.authGeneration === authGen && reply.issue !== null) {
        this.store.linear.saveWrite(params.requestId, op, hash, reply, this.now());
      }
      return reply;
    });
  }

  /**
   * Moves an issue to another workflow state on an explicit person action.
   * Verifies against the cached team states that the target state belongs to the issue's team before sending.
   */
  async updateIssueState(params: {
    issueId: string;
    stateId: string;
    requestId: string;
  }): Promise<{ issue: LinearIssueDetail | null; partial: boolean; warning: string | null }> {
    const op = 'linear_update_issue_state';
    const hash = writeDigest([params.issueId, params.stateId]);
    return this.deduplicatedWrite(params.requestId, op, hash, async () => {
      const { auth, authGen } = await this.requireAuth();
      const teamId = await this.resolveIssueTeamId(params.issueId, auth.api_key, authGen);
      const team = await this.resolveTeamForValidation(teamId, params.stateId, authGen);
      if (!team || !team.states.some(state => state.id === params.stateId)) {
        fail('invalid_request', 'Workflow state does not belong to this issue\'s team.');
      }
      if (this.authGeneration !== authGen) {
        throw new LinearError('not_signed_in', 'Linear sign-in changed while the request was in flight.');
      }
      try {
        await this.client.updateIssueState(auth.api_key, params.issueId, params.stateId);
      } catch (error) {
        this.noteError(error, authGen);
      }
      const provisional = { issue: null as LinearIssueDetail | null, partial: false, warning: null as string | null };
      if (this.authGeneration === authGen) {
        this.cacheGeneration++;
        this.store.linear.invalidateIssue(params.issueId, teamId);
        this.store.linear.saveWrite(params.requestId, op, hash, provisional, this.now());
      }
      const reply = await this.refreshAfterWrite(params.issueId, authGen);
      if (this.authGeneration === authGen) {
        this.store.linear.saveWrite(params.requestId, op, hash, reply, this.now());
      }
      return reply;
    }, async existing => {
      const prior = existing as { issue: LinearIssueDetail | null; partial: boolean; warning: string | null };
      if (prior.issue !== null) return prior;
      const { authGen } = await this.requireAuth();
      const reply = await this.refreshAfterWrite(params.issueId, authGen);
      if (this.authGeneration === authGen && reply.issue !== null) {
        this.store.linear.saveWrite(params.requestId, op, hash, reply, this.now());
      }
      return reply;
    });
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

  private async deduplicatedWrite<T>(
    requestId: string,
    op: string,
    hash: string,
    execute: () => Promise<T>,
    onExisting: (result: unknown) => Promise<T>,
  ): Promise<T> {
    const { authGen } = await this.requireAuth();
    const saved = this.store.linear.getWrite(requestId);
    if (saved) {
      if (saved.op !== op || saved.hash !== hash) {
        fail('conflict', 'Request ID was already used for a different Linear write.');
      }
      return onExisting(saved.result);
    }
    const inflight = this.inFlightWrites.get(requestId);
    if (inflight && inflight.authGen === authGen) {
      if (inflight.op !== op || inflight.hash !== hash) {
        fail('conflict', 'Request ID is already in flight for a different Linear write.');
      }
      return inflight.promise as Promise<T>;
    }
    const promise = execute();
    this.inFlightWrites.set(requestId, { op, hash, authGen, promise });
    try {
      return await promise;
    } finally {
      if (this.inFlightWrites.get(requestId)?.promise === promise) {
        this.inFlightWrites.delete(requestId);
      }
    }
  }
}

function writeDigest(parts: unknown[]): string {
  return createHash('sha256').update(JSON.stringify(parts)).digest('hex');
}
