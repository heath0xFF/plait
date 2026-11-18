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
  private readonly querySequence = new Map<string, number>();

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
      default: fail('invalid_request', 'Unknown operation or field.');
    }
  }

  async authStatus(): Promise<LinearAuthStatus> {
    const auth = await this.authStore.read();
    return formatAuthStatus(auth, this.keyRejected);
  }

  async setKey(apiKey: string): Promise<LinearAuthStatus> {
    const verified = await this.client.viewer(apiKey);
    const saved: StoredLinearAuth = {
      ...verified.data,
      api_key: apiKey,
      saved_at: this.now(),
    };
    await this.authStore.write(saved);
    this.authGeneration++;
    this.cacheGeneration++;
    this.keyRejected = false;
    this.store.linear.clearAll();
    return formatAuthStatus(saved, false);
  }

  async signOut(): Promise<LinearAuthStatus> {
    // Advance generations first so any in-flight read cannot repopulate the cache after sign-out.
    this.authGeneration++;
    this.cacheGeneration++;
    this.keyRejected = false;
    await this.authStore.remove();
    this.store.linear.clearAll();
    return formatAuthStatus(null, false);
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
      if (this.canPopulateCache(scope, seq, authGen, cacheGen)) {
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
    return { checkout: root, binding: this.store.linear.getProjectBinding(root, identity) };
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
      if (this.canPopulateCache(scope, seq, authGen, cacheGen)) {
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
      if (isFirstPage && this.canPopulateCache(scope, seq, authGen, cacheGen)) {
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
}
