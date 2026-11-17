import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { lstat } from 'node:fs/promises';
import type { FileHandle } from 'node:fs/promises';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { clampThinkingLevel, createModels, getSupportedThinkingLevels } from '@earendil-works/pi-ai/models';
import type { Api, Model, Models } from '@earendil-works/pi-ai';
import { createRegistry, Harness, ROOT_CONVERSATION_ID } from '@earendil-works/pi-durable';
import type { Conversation, ConversationRetryPolicy, ConversationWatch, Cursor, EntryQuery, EntryRecord, Submission, SubmissionDraft } from '@earendil-works/pi-durable';
import { SqliteStorage } from '@earendil-works/pi-durable/storage/sqlite';
import { openNodeSqliteDatabase } from '@earendil-works/pi-durable/storage/sqlite/node';
import { fail, failure, queuedView, validate, version, view } from './protocol.ts';
import type { ApprovalMode, ModelChoice, Protocol, QueueMode, Snapshot, InputRecord, ThinkingLevel } from './protocol.ts';
import { Store } from './store.ts';
import { checkout, dirtyPaths, fileAction, identities, acquireCheckout, rootIdentity, checkRoot, pathKey } from './safety.ts';
import { registry } from './tools.ts';
import { discover, expandSkill, skillsPrompt } from './skills.ts';
import type { ToolState } from './tools.ts';
import { MAX_TURNS, update } from './transcript.ts';
import { historyPage, historyText } from './history.ts';
import type { HistorySource } from './history.ts';
import { customTitle } from './text.ts';
import { restoreOwned } from './provenance.ts';
import { RunAuthorization } from './authorization.ts';
import { InteractiveApproval, automaticApproval } from './approval.ts';
import type { Providers } from './providers.ts';
import { commandPolicy, commandPolicyOperation, TRUSTED_INSTRUCTIONS } from './command/policy.ts';

type Session = ToolState & {
  harness?: Harness; root?: Conversation; watch?: ConversationWatch; finished?: Promise<void>;
  inputs: InputRecord[]; requestId: string; approvals: InteractiveApproval; save(): void;
  /** Set once the run's own input is in the conversation store; steering needs a run to join. */
  started?: boolean;
  steered: Map<string, Submission>;
};
type Query = { minEntryId?: number; maxEntryId?: number; order: 'ascending' | 'descending' };
/** Inputs one thread may hold behind its run. */
const QUEUE_LIMIT = 16;
/** Marks a steering input in the conversation store, apart from the ID a later run of its own would use. */
const steerKey = (requestId: string) => `steer:${requestId}`;
function capabilities(model: Model<Api>) {
  return { reasoning: model.reasoning === true, thinking_levels: getSupportedThinkingLevels(model), context_window: model.contextWindow ?? null, input: [...model.input] };
}
export class Runtime {
  store: Store;
  sessions = new Map<string, Session>();
  active = new Map<string, string>();
  models: Models;
  directory: string;
  authenticated: Set<string>;
  // Provider sign-in operations; a reload replaces `models` and `authenticated` together.
  providers?: Providers;
  /** Bounded backoff for transient provider failures of a model request. Tool calls are never retried. */
  retry: ConversationRetryPolicy = { enabled: true, maxRetries: 3, baseDelayMs: 2000 };
  maxTurns = MAX_TURNS;
  private closing = false;
  private gates = new Map<string, Promise<unknown>>();
  private listeners = new Set<(id: string) => void>();
  subscribe(listener: (id: string) => void) {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }
  private changed(id: string) { for (const listener of this.listeners) listener(id); }
  /** `#` tells thread-list watchers that some summary changed; snapshot text alone does not. */
  private notify(id: string, summary: boolean) { this.changed(id); if (summary) this.changed('#'); }
  private save(snapshot: Snapshot, head = 0) { this.notify(snapshot.id, this.store.save(snapshot, head)); }
  /** One conversation store has one user at a time: open, close, and history reads take turns. */
  private gate<T>(id: string, work: () => Promise<T>): Promise<T> {
    const result = (this.gates.get(id) ?? Promise.resolve()).then(work, work);
    const tail = result.catch(() => {});
    this.gates.set(id, tail);
    void tail.then(() => { if (this.gates.get(id) === tail) this.gates.delete(id); });
    return result;
  }
  constructor(directory: string, models: Models, authenticated: Set<string>) {
    this.directory = directory; this.models = models; this.authenticated = authenticated;
    this.store = new Store(directory);
  }
  catalog(protocol: Protocol = 1) {
    return this.models.getModels().map(model => ({
      provider: model.provider, model_id: model.id, name: model.name, authenticated: this.authenticated.has(model.provider),
      ...(protocol === 3 ? capabilities(model) : {}),
    }));
  }
  model(choice: ModelChoice) {
    if (!this.models.getModel(choice.provider, choice.model_id) || !this.authenticated.has(choice.provider)) {
      fail('model_unavailable', 'Exact model is missing or has no configured authentication. Sign in to its provider in Settings.');
    }
  }
  snapshot(id: string) { return this.sessions.get(id)?.snapshot ?? this.store.get(id); }
  async handle(input: unknown): Promise<unknown> {
    if (this.providers?.handles(input)) return this.providers.handle(input);
    const request = validate(input);
    const protocol = version(request);
    const result = await this.dispatch(request, protocol);
    // Every snapshot leaves through the negotiated wire shape.
    if (result && typeof result === 'object' && 'thread' in result) return { ...result, thread: view(result.thread as Snapshot, protocol) };
    return result;
  }
  private async dispatch(request: ReturnType<typeof validate>, protocol: Protocol): Promise<unknown> {
    switch (request.op) {
      case 'status': return { protocol_version: protocol, default_model: this.store.defaultModel(), models: this.catalog(protocol) };
      case 'set_default_model': {
        const choice = { provider: String(request.provider), model_id: String(request.model_id) };
        this.model(choice); this.store.setDefault(choice); return { default_model: choice };
      }
      case 'list_threads': {
        if (protocol !== 3) return { threads: this.store.list(await checkout(String(request.checkout))) };
        const root = request.checkout === undefined ? undefined : await checkout(String(request.checkout));
        return this.store.summaries({ checkout: root, limit: Number(request.limit ?? 100), cursor: request.cursor as string | undefined, archived: request.archived === true });
      }
      // The host repeats this read after each summary change and sends what differs.
      case 'watch_threads': return { threads: this.store.summaries({ limit: 200, archived: request.archived === true }).threads };
      case 'create_thread': {
        const root = await checkout(String(request.checkout));
        const choice = { provider: String(request.provider), model_id: String(request.model_id) };
        const requested = request.thinking as ThinkingLevel | undefined;
        // Appended only when given, so a retried request from an older client matches its record.
        const input = [root, choice.provider, choice.model_id, ...(requested === undefined ? [] : [requested])];
        const duplicate = this.store.duplicate('create', String(request.request_id), input);
        if (duplicate !== undefined) return duplicate;
        this.model(choice);
        const snapshot: Snapshot = { id: randomUUID(), checkout: root, model: choice, status: 'idle', messages: [], live_text: '', tools: [], pending_approval: null, error: null, truncated: false };
        // A starting level is a preference, not a demand: use the nearest one the model has.
        if (requested !== undefined) snapshot.thinking_level = clampThinkingLevel(this.models.getModel(choice.provider, choice.model_id)!, requested);
        const result = { thread: snapshot };
        this.store.admit('create', String(request.request_id), input, result, snapshot, await rootIdentity(root));
        this.changed('#');
        return result;
      }
      case 'watch': return { thread: this.snapshot(String(request.thread_id)) };
      case 'submit': return this.submit(String(request.thread_id), String(request.request_id), String(request.text),
        request.approval === 'auto' ? 'auto' : 'ask', request.when_busy as QueueMode | undefined);
      case 'stop': {
        const id = String(request.thread_id);
        const snapshot = this.snapshot(id);
        const session = this.sessions.get(id);
        snapshot.status = 'stopped'; snapshot.pending_approval = null;
        session?.authorization.revoke();
        // Stop also discards everything waiting behind the run.
        if (this.store.cancel(id)) snapshot.queued = [];
        this.save(snapshot);
        if (session) void session.root?.abort(context).catch(() => {});
        return { thread: snapshot };
      }
      case 'approve': {
        const session = this.sessions.get(String(request.thread_id));
        if (!session) fail('not_found', 'Approval is stale, stopped, or already answered.');
        session.approvals.answer(String(request.approval_id), request.allow === true);
        return { thread: session.snapshot };
      }
      case 'command_policy': case 'set_command_policy': return commandPolicyOperation(this.store, request);
      case 'rename_thread': {
        const snapshot = this.snapshot(String(request.thread_id));
        const title = customTitle(String(request.title));
        if (!title) fail('invalid_request', 'title must contain visible text.');
        snapshot.title = title; snapshot.renamed = true;
        this.save(snapshot);
        return { thread: snapshot };
      }
      case 'archive_thread': {
        const snapshot = this.snapshot(String(request.thread_id));
        snapshot.archived = request.archived === true;
        this.save(snapshot);
        return { thread: snapshot };
      }
      case 'configure_thread': return { thread: this.configure(String(request.thread_id), request) };
      case 'cancel_queued': return { thread: await this.cancelQueued(String(request.thread_id), String(request.request_id)) };
      case 'changed_files': {
        // Files this thread's tools wrote, as recorded for provenance: a list, not a diff.
        const id = String(request.thread_id); this.snapshot(id);
        return { files: this.store.changedPaths(id) };
      }
      case 'history': {
        const id = String(request.thread_id); this.snapshot(id);
        return this.read(id, source => historyPage(source, request.before as string | undefined, Number(request.limit ?? 100)));
      }
      case 'history_text': {
        const id = String(request.thread_id); this.snapshot(id);
        return this.read(id, source => historyText(source, String(request.ref), Number(request.offset)));
      }
      default: fail('invalid_request', 'Unknown operation.');
    }
  }
  /** Changes apply from the next run, so a busy thread refuses them. */
  private configure(id: string, request: ReturnType<typeof validate>) {
    const snapshot = this.snapshot(id);
    if (this.sessions.has(id)) fail('busy', 'Stop the run or wait for it before changing the model or thinking level.');
    const choice = request.provider === undefined ? snapshot.model : { provider: String(request.provider), model_id: String(request.model_id) };
    this.model(choice);
    const model = this.models.getModel(choice.provider, choice.model_id)!;
    const requested = request.thinking as ThinkingLevel | undefined;
    if (requested !== undefined && !getSupportedThinkingLevels(model).includes(requested)) fail('invalid_request', `This model does not support thinking level ${requested}.`);
    // A level the new model lacks moves to its nearest supported one.
    const level = requested ?? clampThinkingLevel(model, snapshot.thinking_level ?? 'off');
    snapshot.model = choice; snapshot.thinking_level = level;
    this.save(snapshot);
    return snapshot;
  }
  private async cancelQueued(id: string, requestId: string) {
    const snapshot = this.snapshot(id);
    const session = this.sessions.get(id);
    const missing = () => fail('not_found', 'Queued message already started or was cancelled.');
    if (!this.store.queue(id).some(input => input.request_id === requestId)) missing();
    const steered = session?.steered.get(requestId);
    // A steering input the run already took is part of the conversation and cannot be withdrawn.
    if (steered && await steered.abort(context).catch(() => 'aborted') === 'already_placed') {
      await this.syncInputs(session!).catch(() => {});
      missing();
    }
    session?.steered.delete(requestId);
    if (!this.store.cancel(id, requestId)) missing();
    snapshot.queued = this.store.queue(id).map(queuedView);
    if (session) session.inputs = this.store.inputs(id);
    this.save(snapshot);
    return snapshot;
  }
  /** Reads a thread's complete conversation without the ability to run anything in it. */
  private read<T>(id: string, work: (source: HistorySource) => Promise<T>): Promise<T> {
    const entries = (root: Conversation) => async (query: Query, limit: number) => {
      const items: EntryRecord[] = []; let cursor: Cursor | undefined;
      do {
        const page = await root.entries(query as unknown as Omit<EntryQuery, 'conversationId'>, limit - items.length, cursor, context);
        items.push(...page.items); cursor = page.next;
      } while (cursor && items.length < limit);
      return items;
    };
    return this.gate(id, async () => {
      const session = this.sessions.get(id);
      const base = {
        pending: (anchor: number, rowid: number, limit: number) => this.store.pendingBefore(id, anchor, rowid, limit),
        placed: (low: number, high: number) => this.store.placed(id, low, high),
        typed: (entry: number) => this.store.placed(id, entry, entry).get(entry)?.text,
        journal: (rowid: number) => this.store.journal(id, rowid), active: !!session,
      };
      if (session?.root) return work({ ...base, entries: entries(session.root) });
      const path = join(this.directory, `${id}.sqlite`);
      if (!existsSync(path)) return work(base);
      // A read-only harness recovers nothing and can never schedule a task, a tool, or a model request.
      const harness = await Harness.open(await SqliteStorage.open(await openNodeSqliteDatabase(path)), { models: createModels(), registry: createRegistry(), readOnly: true }, context);
      try {
        const root = await harness.conversation(ROOT_CONVERSATION_ID, context);
        return await work(root ? { ...base, entries: entries(root) } : base);
      } finally { await harness.close(context); }
    });
  }
  registry(state: ToolState) { return registry(state); }
  /** The person's Pi skills and instructions; a test runtime overrides it to stay independent of the machine. */
  resources(checkout: string, trusted: boolean) { return discover(checkout, trusted, { protect: [this.directory] }); }
  async syncInputs(session: Session, all = false) {
    for (const input of session.inputs) {
      const steered = session.steered.has(input.request_id);
      if (input.entry !== null || (!all && !steered && input.request_id !== session.requestId)) continue;
      // A queued follow-up is not in the conversation store. A steering input is, under its own key.
      if (input.queued && (input.queued.mode !== 'steer' || !(all || steered))) continue;
      const key = input.queued ? steerKey(input.request_id) : input.request_id;
      const record = await session.harness!.commit(tx => tx.submissionByRequest(session.root!.id, key), context);
      if (record?.type === 'input' && record.entry !== undefined) {
        input.entry = Number(record.entry);
        this.store.place(session.snapshot.id, input.request_id, input.entry);
        if (input.queued) { delete input.queued; session.snapshot.queued = this.store.queue(session.snapshot.id).map(queuedView); }
      }
    }
  }
  open(session: Session): Promise<void> {
    return this.gate(session.snapshot.id, () => this.openHarness(session));
  }
  private async openHarness(session: Session): Promise<void> {
    if (session.harness) return;
    const snapshot = session.snapshot;
    // A provider reload swaps the registry; this run keeps the one it started with.
    const models = this.models;
    const database = await openNodeSqliteDatabase(join(this.directory, `${snapshot.id}.sqlite`));
    await database.exec('PRAGMA synchronous=FULL');
    const harness = await Harness.open(await SqliteStorage.open(database), {
      models: () => session.authorization.active ? models : createModels(), registry: this.registry(session),
      // Read at each use. Only model requests retry; a tool call is `replay: unsafe` and runs once.
      settings: () => ({ retry: this.retry, stream: { timeoutMs: 60000 }, toolExecution: 'sequential', steeringMode: 'all' }),
    }, context);
    session.harness = harness;
    session.root = await harness.root(context, { agent: { model: { provider: snapshot.model.provider, modelId: snapshot.model.model_id }, cwd: snapshot.checkout, maxTurns: this.maxTurns } });
    // Fence recovery before enabling scheduling: no model or tool can run here.
    const inspection = await harness.inspect(context);
    for (const task of inspection.tasks) await harness.abortTask(task.record.id, context);
    await session.root.abort(context);
    await this.syncInputs(session, true);
    session.watch = await session.root.watch(context);
    session.watch.start(async call => {
      if (call.kind !== 'value') return;
      await this.syncInputs(session);
      update(snapshot, call.value, session.inputs, this.models.getModel(snapshot.model.provider, snapshot.model.model_id)?.contextWindow ?? null);
      this.save(snapshot, Math.max(0, ...call.value.entries.map(entry => Number(entry.id))));
    });
  }
  async submit(id: string, requestId: string, text: string, mode: ApprovalMode = 'ask', whenBusy?: QueueMode) {
    const snapshot = this.snapshot(id);
    const duplicate = this.store.duplicate(id, requestId, text);
    if (duplicate !== undefined) return duplicate;
    this.model(snapshot.model);
    const result = { thread_id: id, request_id: requestId, accepted: true };
    const session = this.sessions.get(id);
    if (whenBusy && (session || this.store.queue(id).length)) {
      // An idle thread with a held queue resumes it now, which needs the checkout as any run does.
      if (!session && this.active.has(snapshot.checkout)) fail('busy', 'This checkout already has an active run.');
      if (this.store.queue(id).length >= QUEUE_LIMIT) fail('busy', `This thread already has ${QUEUE_LIMIT} queued messages.`);
      this.notify(id, this.store.enqueue(requestId, text, result, { mode: whenBusy, approval: mode }, snapshot));
      if (!session) this.drain(id, true);
      else { session.inputs = this.store.inputs(id); await this.steer(session); }
      return result;
    }
    await this.start(snapshot, requestId, text, mode, () => this.store.admit(id, requestId, text, result, snapshot));
    return result;
  }
  /** Admits one run: reserves the checkout, creates its own authorization, then commits acceptance. */
  private async start(snapshot: Snapshot, requestId: string, text: string, mode: ApprovalMode, accept: () => boolean) {
    const id = snapshot.id;
    if (this.active.has(snapshot.checkout)) fail('busy', 'This checkout already has an active run.');
    if (this.active.size >= 8) fail('busy', 'Host already has eight active checkouts.');
    // Reserve before async baseline reads so two admissions cannot race.
    this.active.set(snapshot.checkout, id);
    let accepted = false;
    let lease: FileHandle | undefined;
    let authorization: RunAuthorization | undefined;
    try {
      await checkRoot(snapshot.checkout, this.store.rootIdentity(id));
      lease = await acquireCheckout(snapshot.checkout);
      const save = () => this.save(snapshot);
      const approvals = new InteractiveApproval(snapshot, save);
      // The mode belongs to this admission only; the next submission chooses again.
      authorization = new RunAuthorization(mode === 'auto' ? automaticApproval : approvals);
      const session: Session = {
        snapshot, baseline: [], protected: [], rootIdentity: this.store.rootIdentity(id), buildCache: { store: this.directory, thread: id }, owned: new Map(), inputs: [], requestId,
        authorization, approvals, lease, save, outputs: this.store.outputs, record: image => this.store.record(id, image), steered: new Map(),
      };
      // Read once here: the run keeps this policy, and a later change waits for the next run.
      session.commandPolicy = commandPolicy(this.store, snapshot.checkout, session.rootIdentity);
      session.forget = path => this.store.forget(id, path);
      this.sessions.set(id, session);
      snapshot.status = 'running'; snapshot.error = null; snapshot.live_text = ''; snapshot.pending_approval = null;
      snapshot.live_reasoning = ''; snapshot.retry = null;
      snapshot.approval_mode = mode;
      this.notify(id, accept());
      session.inputs = this.store.inputs(id); session.requestId = requestId;
      accepted = true;
      session.finished = this.run(session, requestId, text);
    } finally {
      if (!accepted) {
        authorization?.revoke();
        if (authorization) this.sessions.delete(id);
        await lease?.close(); this.active.delete(snapshot.checkout);
      }
    }
  }
  /** Offers queued steering inputs to the run that is active now, under that run's authorization. */
  private async steer(session: Session) {
    if (!session.started || !session.root || session.authorization.signal.aborted) return;
    for (const input of session.inputs) {
      if (input.queued?.mode !== 'steer' || session.steered.has(input.request_id)) continue;
      try {
        session.steered.set(input.request_id, await session.root.submit(
          { type: 'input', requestId: steerKey(input.request_id), content: await expandSkill(input.text, session.skills ?? []), whenBusy: 'steer', whenIdle: 'reject' }, context));
      } catch { /* The run is ending. The input stays queued and starts a run of its own. */ }
    }
  }
  /** Starts the next queued input as a new run with a new authorization in its own mode. */
  private drain(id: string, resume = false) {
    if (this.closing || this.sessions.has(id)) return;
    const snapshot = this.store.get(id);
    // A failed or interrupted run holds its queue. Stop clears it, so anything left after a stop was sent later.
    if (!resume && snapshot.status !== 'idle' && snapshot.status !== 'stopped') return;
    const next = this.store.queue(id)[0];
    if (!next) return;
    const begin = async () => {
      this.model(snapshot.model);
      await this.start(snapshot, next.request_id, next.text, next.queued!.approval, () => this.store.dequeue(next, snapshot));
    };
    void begin().catch(error => {
      if (this.sessions.has(id)) return;
      const held = this.store.get(id);
      held.status = 'error'; held.error = failure(error);
      this.save(held);
    });
  }
  /** The run's own input, or a continuation when a crash hid that an earlier run already took it as steering. */
  private async draft(session: Session, requestId: string, text: string): Promise<SubmissionDraft> {
    const earlier = await session.harness!.commit(tx => tx.submissionByRequest(session.root!.id, steerKey(requestId)), context);
    if (earlier?.type !== 'input' || earlier.entry === undefined) return { type: 'input', requestId, content: await expandSkill(text, session.skills ?? []) };
    this.store.place(session.snapshot.id, requestId, Number(earlier.entry));
    session.inputs = this.store.inputs(session.snapshot.id);
    return { type: 'continue' };
  }
  private async release(session: Session) {
    await session.watch?.stop();
    // A steering input the run never took must not wait in the conversation store: it starts its own run later.
    if (session.harness && session.root && session.steered.size) {
      await this.syncInputs(session).catch(() => {});
      for (const submission of session.steered.values()) await submission.abort(context).catch(() => {});
    }
    await session.harness?.close(context);
    session.root = undefined;
  }
  async run(session: Session, requestId: string, text: string) {
    const snapshot = session.snapshot;
    try {
      const baseline = await dirtyPaths(snapshot.checkout);
      session.owned = await restoreOwned(snapshot.checkout, baseline, this.store.owned(snapshot.id), session.rootIdentity, session.lease?.fd);
      session.baseline = baseline.filter(path => !session.owned.has(pathKey(path)));
      session.protected = await identities(snapshot.checkout, session.baseline);
      let instructions = 'You are Plait, a local coding assistant. File tools are the only checkout writers; edits and commands may require the user\'s approval. Commands read the checkout and write only private scratch (HOME/TMPDIR) and this thread\'s private build cache (CARGO_HOME/CARGO_TARGET_DIR), with no external network and a 15 minute limit. Tests may use scratch-local Git repositories and Unix sockets. Use offline locked builds. Unchanged files this thread previously wrote can be edited again. Never modify other dirty or ignored paths, bypass approval, or start daemons. Read project instructions below.\n';
      if (session.commandPolicy === 'trusted') instructions = TRUSTED_INSTRUCTIONS;
      // The person's own Pi setup: user-level instructions, then the project's, then skills.
      // Project skills need the trust that trusted command mode already expresses.
      const resources = await this.resources(snapshot.checkout, session.commandPolicy === 'trusted');
      session.skills = resources.skills;
      if (resources.instructions) instructions += `\nUser instructions (${resources.instructions.path}):\n` + resources.instructions.text;
      try {
        await lstat(join(snapshot.checkout, 'AGENTS.md'));
        const { text: project } = await fileAction(snapshot.checkout, 'read', 'AGENTS.md', undefined, session.lease?.fd, session.rootIdentity);
        if (project.length > 32768) fail('permission_denied', 'AGENTS.md exceeds the instruction bound.');
        instructions += '\nAGENTS.md:\n' + project;
      } catch (error) { if ((error as { code?: string }).code !== 'ENOENT') throw error; }
      instructions += skillsPrompt(session.skills);
      await this.open(session);
      // The thread's current model, thinking level, and turn limit apply from this run.
      await session.root!.configure({
        instructions, model: { provider: snapshot.model.provider, modelId: snapshot.model.model_id },
        thinkingLevel: snapshot.thinking_level ?? null, maxTurns: this.maxTurns,
      }, context);
      if (session.authorization.signal.aborted) return;
      session.authorization.activate();
      const submission = await session.root!.submit(await this.draft(session, requestId, text), context);
      session.started = true;
      await this.steer(session);
      await submission.wait(context);
      await session.root!.waitForIdle(context);
    } catch (error) {
      if (snapshot.status !== 'stopped' && snapshot.status !== 'interrupted') {
        snapshot.error = failure(error);
      }
    } finally {
      session.authorization.revoke();
      snapshot.pending_approval = null; snapshot.live_text = ''; snapshot.live_reasoning = ''; snapshot.retry = null;
      await this.gate(snapshot.id, () => this.release(session));
      await session.lease?.close(); session.lease = undefined;
      this.active.delete(snapshot.checkout);
      // Idle means a following submit can acquire the checkout immediately.
      if (snapshot.status !== 'stopped' && snapshot.status !== 'interrupted') snapshot.status = snapshot.error ? 'error' : 'idle';
      session.save(); this.sessions.delete(snapshot.id);
      this.drain(snapshot.id);
    }
  }
  async close() {
    this.closing = true;
    for (const session of this.sessions.values()) {
      if (this.active.has(session.snapshot.checkout)) {
        session.snapshot.status = 'interrupted'; session.snapshot.pending_approval = null;
        session.snapshot.error = { code: 'interrupted', message: 'Host stopped. Inspect effects before a new submission.' }; session.save();
      }
      session.authorization.revoke();
    }
    for (const session of this.sessions.values()) {
      await session.harness?.close(context);
      await session.finished;
      await session.harness?.close(context);
    }
    await this.providers?.close();
    this.store.close();
  }
  async reply(input: unknown) {
    try { return { ok: true, result: await this.handle(input) }; }
    catch (error) { return { ok: false, error: failure(error) }; }
  }
}
