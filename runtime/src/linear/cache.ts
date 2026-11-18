import type { DatabaseSync } from 'node:sqlite';
import type { FileIdentity } from '../safety.ts';
import {
  LINEAR_CACHE_MAX_DETAILS,
  LINEAR_CACHE_MAX_ISSUES_PER_TEAM,
  LINEAR_CACHE_MAX_PAGES,
} from './types.ts';
import type {
  LinearIssueDetail,
  LinearIssueSummary,
  LinearProjectBinding,
  LinearTeam,
} from './types.ts';

export type CachedTeams = { teams: LinearTeam[]; cached_at: number };
export type CachedIssuesPage = {
  issues: LinearIssueSummary[];
  has_more: boolean;
  next_cursor: string | null;
  cached_at: number;
};
export type CachedIssueDetail = { issue: LinearIssueDetail; cached_at: number };

const TEAMS_META_KEY = 'teams';

/**
 * Additive SQLite persistence for Linear project bindings and bounded API caches.
 * Credentials are never stored here; they live exclusively in `$STORE/linear-auth.json`.
 */
export class LinearCache {
  private readonly db: DatabaseSync;
  private touchSeq = 0;

  constructor(db: DatabaseSync) {
    this.db = db;
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS linear_projects(
        canonical_root TEXT NOT NULL,
        root_dev TEXT NOT NULL,
        root_ino TEXT NOT NULL,
        workspace_id TEXT,
        team_id TEXT NOT NULL,
        team_key TEXT,
        team_name TEXT,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(canonical_root, root_dev, root_ino)
      );
      CREATE TABLE IF NOT EXISTS linear_cache_meta(
        key TEXT PRIMARY KEY,
        cached_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS linear_cache_teams(
        team_id TEXT PRIMARY KEY,
        payload TEXT NOT NULL,
        cached_at INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS linear_cache_issues(
        team_id TEXT NOT NULL,
        issue_id TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        cached_at INTEGER NOT NULL,
        touched_seq INTEGER NOT NULL DEFAULT 0,
        payload TEXT NOT NULL,
        PRIMARY KEY(team_id, issue_id)
      );
      CREATE INDEX IF NOT EXISTS linear_cache_issues_team_recent
        ON linear_cache_issues(team_id, cached_at DESC, touched_seq DESC, updated_at DESC);
      CREATE TABLE IF NOT EXISTS linear_cache_pages(
        cache_key TEXT PRIMARY KEY,
        team_id TEXT NOT NULL,
        issue_ids TEXT NOT NULL,
        has_more INTEGER NOT NULL,
        next_cursor TEXT,
        cached_at INTEGER NOT NULL,
        accessed_at INTEGER NOT NULL
      );
      CREATE INDEX IF NOT EXISTS linear_cache_pages_team ON linear_cache_pages(team_id);
      CREATE TABLE IF NOT EXISTS linear_cache_details(
        issue_id TEXT PRIMARY KEY,
        team_id TEXT NOT NULL,
        payload TEXT NOT NULL,
        cached_at INTEGER NOT NULL,
        accessed_at INTEGER NOT NULL
      );
    `);
  }

  private transaction<T>(work: () => T): T {
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const result = work();
      this.db.exec('COMMIT');
      return result;
    } catch (error) {
      this.db.exec('ROLLBACK');
      throw error;
    }
  }

  getProjectBinding(root: string, identity: FileIdentity): LinearProjectBinding | null {
    const row = this.db.prepare(
      'SELECT workspace_id, team_id, team_key, team_name, updated_at FROM linear_projects WHERE canonical_root=? AND root_dev=? AND root_ino=?',
    ).get(root, identity.dev, identity.ino);
    if (!row) return null;
    return {
      workspace_id: typeof row.workspace_id === 'string' ? row.workspace_id : null,
      team_id: String(row.team_id),
      team_key: typeof row.team_key === 'string' ? row.team_key : null,
      team_name: typeof row.team_name === 'string' ? row.team_name : null,
      updated_at: Number(row.updated_at),
    };
  }

  setProjectBinding(root: string, identity: FileIdentity, binding: LinearProjectBinding | null): void {
    if (!binding) {
      this.db.prepare('DELETE FROM linear_projects WHERE canonical_root=? AND root_dev=? AND root_ino=?')
        .run(root, identity.dev, identity.ino);
      return;
    }
    this.db.prepare(`
      INSERT INTO linear_projects(canonical_root, root_dev, root_ino, workspace_id, team_id, team_key, team_name, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      ON CONFLICT(canonical_root, root_dev, root_ino) DO UPDATE SET
        workspace_id=excluded.workspace_id,
        team_id=excluded.team_id,
        team_key=excluded.team_key,
        team_name=excluded.team_name,
        updated_at=excluded.updated_at
    `).run(root, identity.dev, identity.ino, binding.workspace_id, binding.team_id, binding.team_key, binding.team_name, binding.updated_at);
  }

  getTeams(): CachedTeams | null {
    const meta = this.db.prepare('SELECT cached_at FROM linear_cache_meta WHERE key=?').get(TEAMS_META_KEY);
    if (!meta) return null;
    const rows = this.db.prepare('SELECT payload FROM linear_cache_teams ORDER BY rowid ASC').all();
    return {
      teams: rows.map(row => JSON.parse(String(row.payload)) as LinearTeam),
      cached_at: Number(meta.cached_at),
    };
  }

  getTeam(teamId: string): LinearTeam | null {
    const row = this.db.prepare('SELECT payload FROM linear_cache_teams WHERE team_id=?').get(teamId);
    return row ? JSON.parse(String(row.payload)) as LinearTeam : null;
  }

  saveTeams(teams: LinearTeam[], cachedAt: number): void {
    this.transaction(() => {
      this.db.prepare('DELETE FROM linear_cache_teams').run();
      const insert = this.db.prepare('INSERT INTO linear_cache_teams(team_id, payload, cached_at) VALUES (?, ?, ?)');
      for (const team of teams) insert.run(team.id, JSON.stringify(team), cachedAt);
      this.db.prepare('INSERT INTO linear_cache_meta(key, cached_at) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET cached_at=excluded.cached_at')
        .run(TEAMS_META_KEY, cachedAt);
    });
  }

  getIssuesPage(cacheKey: string, now: number): CachedIssuesPage | null {
    const page = this.db.prepare(
      'SELECT team_id, issue_ids, has_more, next_cursor, cached_at FROM linear_cache_pages WHERE cache_key=?',
    ).get(cacheKey);
    if (!page) return null;
    const teamId = String(page.team_id);
    const ids = JSON.parse(String(page.issue_ids)) as string[];
    const select = this.db.prepare('SELECT payload FROM linear_cache_issues WHERE team_id=? AND issue_id=?');
    const issues: LinearIssueSummary[] = [];
    for (const issueId of ids) {
      const row = select.get(teamId, issueId);
      // If any issue in the cached page was evicted by the per-team cap, do not serve a holey page.
      if (!row) return null;
      issues.push(JSON.parse(String(row.payload)) as LinearIssueSummary);
    }
    this.db.prepare('UPDATE linear_cache_pages SET accessed_at=? WHERE cache_key=?').run(now, cacheKey);
    return {
      issues,
      has_more: page.has_more === 1,
      next_cursor: typeof page.next_cursor === 'string' ? page.next_cursor : null,
      cached_at: Number(page.cached_at),
    };
  }

  saveIssuesPage(
    cacheKey: string,
    teamId: string,
    issues: LinearIssueSummary[],
    hasMore: boolean,
    nextCursor: string | null,
    cachedAt: number,
  ): void {
    this.transaction(() => {
      const seq = ++this.touchSeq;
      const upsertIssue = this.db.prepare(`
        INSERT INTO linear_cache_issues(team_id, issue_id, updated_at, cached_at, touched_seq, payload)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(team_id, issue_id) DO UPDATE SET
          updated_at=excluded.updated_at,
          cached_at=excluded.cached_at,
          touched_seq=excluded.touched_seq,
          payload=excluded.payload
      `);
      for (const issue of issues) {
        upsertIssue.run(teamId, issue.id, issue.updated_at, cachedAt, seq, JSON.stringify(issue));
      }
      // Strictly bound cached summaries to LINEAR_CACHE_MAX_ISSUES_PER_TEAM per team.
      this.db.prepare(`
        DELETE FROM linear_cache_issues
        WHERE team_id=? AND issue_id NOT IN (
          SELECT issue_id FROM linear_cache_issues
          WHERE team_id=?
          ORDER BY cached_at DESC, touched_seq DESC, updated_at DESC, issue_id ASC
          LIMIT ?
        )
      `).run(teamId, teamId, LINEAR_CACHE_MAX_ISSUES_PER_TEAM);

      this.db.prepare(`
        INSERT INTO linear_cache_pages(cache_key, team_id, issue_ids, has_more, next_cursor, cached_at, accessed_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(cache_key) DO UPDATE SET
          team_id=excluded.team_id,
          issue_ids=excluded.issue_ids,
          has_more=excluded.has_more,
          next_cursor=excluded.next_cursor,
          cached_at=excluded.cached_at,
          accessed_at=excluded.accessed_at
      `).run(cacheKey, teamId, JSON.stringify(issues.map(issue => issue.id)), hasMore ? 1 : 0, nextCursor, cachedAt, cachedAt);

      this.db.prepare(`
        DELETE FROM linear_cache_pages
        WHERE cache_key NOT IN (
          SELECT cache_key FROM linear_cache_pages ORDER BY accessed_at DESC, cache_key ASC LIMIT ?
        )
      `).run(LINEAR_CACHE_MAX_PAGES);
    });
  }

  getIssueDetail(issueId: string, now: number): CachedIssueDetail | null {
    const row = this.db.prepare('SELECT payload, cached_at FROM linear_cache_details WHERE issue_id=?').get(issueId);
    if (!row) return null;
    this.db.prepare('UPDATE linear_cache_details SET accessed_at=? WHERE issue_id=?').run(now, issueId);
    return {
      issue: JSON.parse(String(row.payload)) as LinearIssueDetail,
      cached_at: Number(row.cached_at),
    };
  }

  saveIssueDetail(issue: LinearIssueDetail, cachedAt: number): void {
    this.transaction(() => {
      this.db.prepare(`
        INSERT INTO linear_cache_details(issue_id, team_id, payload, cached_at, accessed_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(issue_id) DO UPDATE SET
          team_id=excluded.team_id,
          payload=excluded.payload,
          cached_at=excluded.cached_at,
          accessed_at=excluded.accessed_at
      `).run(issue.id, issue.team_id, JSON.stringify(issue), cachedAt, cachedAt);

      // Keep LRU cap of LINEAR_CACHE_MAX_DETAILS issue details.
      this.db.prepare(`
        DELETE FROM linear_cache_details
        WHERE issue_id NOT IN (
          SELECT issue_id FROM linear_cache_details ORDER BY accessed_at DESC, issue_id ASC LIMIT ?
        )
      `).run(LINEAR_CACHE_MAX_DETAILS);

      // Keep matching summary row fresh if the issue belongs to a team.
      if (issue.team_id) {
        const summary: LinearIssueSummary = {
          id: issue.id,
          identifier: issue.identifier,
          title: issue.title,
          url: issue.url,
          priority: issue.priority,
          updated_at: issue.updated_at,
          team_id: issue.team_id,
          state: issue.state,
          assignee: issue.assignee,
          labels: issue.labels,
        };
        this.db.prepare(`
          UPDATE linear_cache_issues
          SET updated_at=?, cached_at=?, payload=?
          WHERE team_id=? AND issue_id=?
        `).run(summary.updated_at, cachedAt, JSON.stringify(summary), summary.team_id, summary.id);
      }
    });
  }

  /** Looks up the cached team_id for an issue, checking detail cache then summary cache. */
  findIssueTeamId(issueId: string): string | null {
    const detail = this.db.prepare('SELECT team_id FROM linear_cache_details WHERE issue_id=?').get(issueId);
    if (detail && typeof detail.team_id === 'string' && detail.team_id) return detail.team_id;
    const summary = this.db.prepare('SELECT team_id FROM linear_cache_issues WHERE issue_id=? LIMIT 1').get(issueId);
    if (summary && typeof summary.team_id === 'string' && summary.team_id) return summary.team_id;
    return null;
  }

  /** Invalidates cached detail for an issue and cached pages for its team. */
  invalidateIssue(issueId: string, teamId?: string | null): void {
    this.transaction(() => {
      const resolvedTeam = teamId ?? this.findIssueTeamId(issueId);
      this.db.prepare('DELETE FROM linear_cache_details WHERE issue_id=?').run(issueId);
      if (resolvedTeam) {
        this.db.prepare('DELETE FROM linear_cache_pages WHERE team_id=?').run(resolvedTeam);
      } else {
        this.db.prepare('DELETE FROM linear_cache_pages').run();
      }
    });
  }

  /** Clears all cached Linear API responses (e.g. on sign-out or switching keys). */
  clearAll(): void {
    this.transaction(() => {
      this.db.prepare('DELETE FROM linear_cache_meta').run();
      this.db.prepare('DELETE FROM linear_cache_teams').run();
      this.db.prepare('DELETE FROM linear_cache_issues').run();
      this.db.prepare('DELETE FROM linear_cache_pages').run();
      this.db.prepare('DELETE FROM linear_cache_details').run();
    });
  }
}
