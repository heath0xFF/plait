import { randomUUID } from 'node:crypto';
import { lstat } from 'node:fs/promises';
import type { FileHandle } from 'node:fs/promises';
import { join } from 'node:path';
import { BACKGROUND_CONTEXT as context } from '@earendil-works/chord/context';
import { createModels } from '@earendil-works/pi-ai/models';
import type { Models } from '@earendil-works/pi-ai';
import { Harness } from '@earendil-works/pi-durable';
import type { Conversation, ConversationWatch } from '@earendil-works/pi-durable';
import { SqliteStorage } from '@earendil-works/pi-durable/storage/sqlite';
import { openNodeSqliteDatabase } from '@earendil-works/pi-durable/storage/sqlite/node';
import { fail, failure, validate, version, view } from './protocol.ts';
import type { ApprovalMode, ModelChoice, Snapshot, InputRecord } from './protocol.ts';
import { Store } from './store.ts';
import { checkout, dirtyPaths, fileAction, identities, acquireCheckout, rootIdentity, checkRoot, pathKey } from './safety.ts';
import { registry } from './tools.ts';
import type { ToolState } from './tools.ts';
import { update } from './transcript.ts';
import { restoreOwned } from './provenance.ts';
import { RunAuthorization } from './authorization.ts';
import { InteractiveApproval, automaticApproval } from './approval.ts';
import type { Providers } from './providers.ts';
import { commandPolicy, commandPolicyOperation, TRUSTED_INSTRUCTIONS } from './command/policy.ts';

type Session = ToolState & {
  harness?: Harness; root?: Conversation; watch?: ConversationWatch; finished?: Promise<void>;
  inputs: InputRecord[]; requestId: string; approvals: InteractiveApproval; save(): void;
};
export class Runtime {
  store: Store;
  sessions = new Map<string, Session>();
  active = new Map<string, string>();
  models: Models;
  directory: string;
  authenticated: Set<string>;
  // Provider sign-in operations; a reload replaces `models` and `authenticated` together.
  providers?: Providers;
  private listeners = new Set<(id: string) => void>();
  subscribe(listener: (id: string) => void) {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }
  private changed(id: string) { for (const listener of this.listeners) listener(id); }
  private save(snapshot: Snapshot, head = 0) {
    this.store.save(snapshot, head);
    this.changed(snapshot.id);
  }
  constructor(directory: string, models: Models, authenticated: Set<string>) {
    this.directory = directory; this.models = models; this.authenticated = authenticated;
    this.store = new Store(directory);
  }
  catalog() {
    return this.models.getModels().map(model => ({ provider: model.provider, model_id: model.id, name: model.name, authenticated: this.authenticated.has(model.provider) }));
  }
  model(choice: ModelChoice) {
    if (!this.models.getModel(choice.provider, choice.model_id) || !this.authenticated.has(choice.provider)) {
      fail('model_unavailable', 'Exact model is missing or has no configured authentication. Configure it in Pi, then restart the host.');
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
  private async dispatch(request: ReturnType<typeof validate>, protocol: 1 | 2): Promise<unknown> {
    switch (request.op) {
      case 'status': return { protocol_version: protocol, default_model: this.store.defaultModel(), models: this.catalog() };
      case 'set_default_model': {
        const choice = { provider: String(request.provider), model_id: String(request.model_id) };
        this.model(choice); this.store.setDefault(choice); return { default_model: choice };
      }
      case 'list_threads': {
        const root = await checkout(String(request.checkout));
        return { threads: this.store.list(root) };
      }
      case 'create_thread': {
        const root = await checkout(String(request.checkout));
        const choice = { provider: String(request.provider), model_id: String(request.model_id) };
        const input = [root, choice.provider, choice.model_id];
        const duplicate = this.store.duplicate('create', String(request.request_id), input);
        if (duplicate !== undefined) return duplicate;
        this.model(choice);
        const snapshot: Snapshot = { id: randomUUID(), checkout: root, model: choice, status: 'idle', messages: [], live_text: '', tools: [], pending_approval: null, error: null, truncated: false };
        const result = { thread: snapshot };
        this.store.admit('create', String(request.request_id), input, result, snapshot, await rootIdentity(root));
        return result;
      }
      case 'watch': return { thread: this.snapshot(String(request.thread_id)) };
      case 'submit': return this.submit(String(request.thread_id), String(request.request_id), String(request.text),
        request.approval === 'auto' ? 'auto' : 'ask');
      case 'stop': {
        const id = String(request.thread_id);
        const snapshot = this.snapshot(id);
        const session = this.sessions.get(id);
        snapshot.status = 'stopped'; snapshot.pending_approval = null;
        session?.authorization.revoke();
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
      default: fail('invalid_request', 'Unknown operation.');
    }
  }
  registry(state: ToolState) { return registry(state); }
  async syncInputs(session: Session, all = false) {
    for (const input of session.inputs) {
      if (input.entry !== null || (!all && input.request_id !== session.requestId)) continue;
      const record = await session.harness!.commit(tx => tx.submissionByRequest(session.root!.id, input.request_id), context);
      if (record?.type === 'input' && record.entry !== undefined) {
        input.entry = Number(record.entry);
        this.store.place(session.snapshot.id, input.request_id, input.entry);
      }
    }
  }
  async open(session: Session): Promise<void> {
    if (session.harness) return;
    const snapshot = session.snapshot;
    // A provider reload swaps the registry; this run keeps the one it started with.
    const models = this.models;
    const database = await openNodeSqliteDatabase(join(this.directory, `${snapshot.id}.sqlite`));
    await database.exec('PRAGMA synchronous=FULL');
    const harness = await Harness.open(await SqliteStorage.open(database), {
      models: () => session.authorization.active ? models : createModels(), registry: this.registry(session),
      settings: { retry: { maxRetries: 0 }, stream: { timeoutMs: 60000 }, toolExecution: 'sequential' },
    }, context);
    session.harness = harness;
    session.root = await harness.root(context, { agent: { model: { provider: snapshot.model.provider, modelId: snapshot.model.model_id }, cwd: snapshot.checkout, maxTurns: 20 } });
    // Fence recovery before enabling scheduling: no model or tool can run here.
    const inspection = await harness.inspect(context);
    for (const task of inspection.tasks) await harness.abortTask(task.record.id, context);
    await session.root.abort(context);
    await this.syncInputs(session, true);
    session.watch = await session.root.watch(context);
    session.watch.start(async call => {
      if (call.kind !== 'value') return;
      await this.syncInputs(session);
      update(snapshot, call.value, session.inputs);
      this.save(snapshot, Math.max(0, ...call.value.entries.map(entry => Number(entry.id))));
    });
  }
  async submit(id: string, requestId: string, text: string, mode: ApprovalMode = 'ask') {
    const snapshot = this.snapshot(id);
    const duplicate = this.store.duplicate(id, requestId, text);
    if (duplicate !== undefined) return duplicate;
    this.model(snapshot.model);
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
        authorization, approvals, lease, save, record: image => this.store.record(id, image),
      };
      // Read once here: the run keeps this policy, and a later change waits for the next run.
      session.commandPolicy = commandPolicy(this.store, snapshot.checkout, session.rootIdentity);
      session.forget = path => this.store.forget(id, path);
      this.sessions.set(id, session);
      snapshot.status = 'running'; snapshot.error = null; snapshot.live_text = ''; snapshot.pending_approval = null;
      snapshot.approval_mode = mode;
      const result = { thread_id: id, request_id: requestId, accepted: true };
      this.store.admit(id, requestId, text, result, snapshot);
      this.changed(id);
      session.inputs = this.store.inputs(id); session.requestId = requestId;
      accepted = true;
      session.finished = this.run(session, requestId, text);
      return result;
    } finally {
      if (!accepted) {
        authorization?.revoke();
        if (authorization) this.sessions.delete(id);
        await lease?.close(); this.active.delete(snapshot.checkout);
      }
    }
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
      try {
        await lstat(join(snapshot.checkout, 'AGENTS.md'));
        const { text: project } = await fileAction(snapshot.checkout, 'read', 'AGENTS.md', undefined, session.lease?.fd, session.rootIdentity);
        if (project.length > 32768) fail('permission_denied', 'AGENTS.md exceeds the instruction bound.');
        instructions += '\nAGENTS.md:\n' + project;
      } catch (error) { if ((error as { code?: string }).code !== 'ENOENT') throw error; }
      await this.open(session);
      await session.root!.configure({ instructions }, context);
      if (session.authorization.signal.aborted) return;
      session.authorization.activate();
      const submission = await session.root!.submit({ type: 'input', requestId, content: text }, context);
      await submission.wait(context);
      await session.root!.waitForIdle(context);
    } catch (error) {
      if (snapshot.status !== 'stopped' && snapshot.status !== 'interrupted') {
        snapshot.error = failure(error);
      }
    } finally {
      session.authorization.revoke();
      snapshot.pending_approval = null; snapshot.live_text = '';
      await session.watch?.stop();
      await session.harness?.close(context);
      await session.lease?.close(); session.lease = undefined;
      this.active.delete(snapshot.checkout);
      // Idle means a following submit can acquire the checkout immediately.
      if (snapshot.status !== 'stopped' && snapshot.status !== 'interrupted') snapshot.status = snapshot.error ? 'error' : 'idle';
      session.save(); this.sessions.delete(snapshot.id);
    }
  }
  async close() {
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
