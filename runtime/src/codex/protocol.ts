/**
 * The subset of the Codex app-server protocol Plait uses, written by hand and
 * checked field by field against the TypeScript generated from codex 0.169.0
 * (`codex app-server generate-ts`). Each type names the generated file it
 * mirrors. Fields Plait never reads or sends are left out; responses can carry
 * more fields than listed here.
 *
 * Wire format (verified against codex-cli 0.169.0): one JSON object per line on
 * stdio, JSON-RPC 2.0 without the `"jsonrpc"` member in server output (the
 * server accepts it in client input). `params` is required on every request.
 * Requests other than `initialize` fail with -32600 "Not initialized" until
 * `initialize` has been answered; a second `initialize` fails with "Already
 * initialized". The client then sends the `initialized` notification
 * (ClientNotification.ts). A line that is not JSON is ignored without a reply.
 */

export const CODEX_SCHEMA_VERSION = '0.169.0';

// RequestId.ts
export type RequestId = string | number;

// --- Handshake ---------------------------------------------------------------

// ClientInfo.ts
export type ClientInfo = { name: string; title: string | null; version: string };
// InitializeCapabilities.ts (optional members omitted)
export type InitializeCapabilities = { experimentalApi: boolean; requestAttestation: boolean; optOutNotificationMethods?: string[] | null };
// InitializeParams.ts
export type InitializeParams = { clientInfo: ClientInfo; capabilities: InitializeCapabilities | null };
// InitializeResponse.ts
export type InitializeResponse = { userAgent: string; codexHome: string; platformFamily: string; platformOs: string };

// --- Policies ----------------------------------------------------------------

// ReasoningEffort.ts: an open string; model/list names the supported ones.
export type ReasoningEffort = string;
// ReasoningSummary.ts
export type ReasoningSummary = 'auto' | 'concise' | 'detailed' | 'none';
// v2/AskForApproval.ts (the `granular` object form is not used by Plait)
export type AskForApproval = 'untrusted' | 'on-request' | 'never';
// v2/SandboxMode.ts: thread/start and thread/resume take a mode.
export type SandboxMode = 'read-only' | 'workspace-write' | 'danger-full-access';
// v2/SandboxPolicy.ts: turn/start takes a full policy. (`externalSandbox` omitted.)
export type SandboxPolicy =
  | { type: 'dangerFullAccess' }
  | { type: 'readOnly'; networkAccess: boolean }
  | { type: 'workspaceWrite'; writableRoots: string[]; networkAccess: boolean; excludeTmpdirEnvVar: boolean; excludeSlashTmp: boolean };

// --- Threads and turns -------------------------------------------------------

// v2/TextElement.ts and v2/ByteRange.ts are not needed: Plait sends none.
// v2/UserInput.ts (text variant only)
export type UserInput = { type: 'text'; text: string; text_elements: [] } | { type: 'image'; url: string };

// v2/ThreadStartParams.ts
// v2/ApprovalsReviewer.ts
export type ApprovalsReviewer = 'user' | 'auto_review' | 'guardian_subagent';
export type ThreadStartParams = {
  model?: string | null; cwd?: string | null; approvalPolicy?: AskForApproval | null; sandbox?: SandboxMode | null;
  approvalsReviewer?: ApprovalsReviewer | null;
  baseInstructions?: string | null; developerInstructions?: string | null; ephemeral?: boolean | null;
};
// v2/ThreadResumeParams.ts
export type ThreadResumeParams = {
  threadId: string; model?: string | null; cwd?: string | null; approvalPolicy?: AskForApproval | null; sandbox?: SandboxMode | null;
  approvalsReviewer?: ApprovalsReviewer | null;
  developerInstructions?: string | null;
  /** Return thread metadata without `thread.turns`. */
  excludeTurns?: boolean;
};
// v2/ThreadStatus.ts
export type ThreadStatus = { type: 'notLoaded' } | { type: 'idle' } | { type: 'systemError' } | { type: 'active'; activeFlags: string[] };
// v2/Thread.ts (subset)
export type Thread = {
  id: string; preview: string; ephemeral: boolean; modelProvider: string; model: string | null; reasoningEffort: ReasoningEffort | null;
  createdAt: number; updatedAt: number; status: ThreadStatus; cwd: string; cliVersion: string; name: string | null; turns: Turn[];
};
// v2/ThreadStartResponse.ts (subset)
export type ThreadStartResponse = {
  thread: Thread; model: string; modelProvider: string; cwd: string; approvalPolicy: AskForApproval | { granular: unknown };
  sandbox: SandboxPolicy | { type: 'externalSandbox'; networkAccess: unknown }; reasoningEffort: ReasoningEffort | null;
  approvalsReviewer?: ApprovalsReviewer;
};
// v2/ThreadResumeResponse.ts (subset; same core fields as ThreadStartResponse)
export type ThreadResumeResponse = ThreadStartResponse;

// v2/TurnStatus.ts
export type TurnStatus = 'completed' | 'interrupted' | 'failed' | 'inProgress';
// v2/TurnError.ts (subset)
export type TurnError = { message: string; additionalDetails: string | null };
// v2/Turn.ts (subset)
export type Turn = {
  id: string; items: ThreadItem[]; status: TurnStatus; error: TurnError | null;
  startedAt: number | null; completedAt: number | null; durationMs: number | null;
};
// v2/TurnStartParams.ts (subset)
export type TurnStartParams = {
  threadId: string; input: UserInput[];
  /** Echoed as `clientId` on the resulting `userMessage` item. */
  clientUserMessageId?: string | null;
  cwd?: string | null; approvalPolicy?: AskForApproval | null; sandboxPolicy?: SandboxPolicy | null;
  approvalsReviewer?: ApprovalsReviewer | null;
  model?: string | null; effort?: ReasoningEffort | null; summary?: ReasoningSummary | null;
};
// v2/TurnStartResponse.ts
export type TurnStartResponse = { turn: Turn };
// v2/TurnSteerParams.ts
export type TurnSteerParams = { threadId: string; clientUserMessageId?: string | null; input: UserInput[]; expectedTurnId: string };
// v2/TurnSteerResponse.ts
export type TurnSteerResponse = { turnId: string };
// v2/TurnInterruptParams.ts
export type TurnInterruptParams = { threadId: string; turnId: string };
// v2/TurnInterruptResponse.ts
export type TurnInterruptResponse = Record<string, never>;

// --- Thread items (v2/ThreadItem.ts) -----------------------------------------

// v2/CommandExecutionStatus.ts and v2/PatchApplyStatus.ts share these values.
export type CommandExecutionStatus = 'inProgress' | 'completed' | 'failed' | 'declined';
export type PatchApplyStatus = CommandExecutionStatus;
// v2/McpToolCallStatus.ts
export type McpToolCallStatus = 'inProgress' | 'completed' | 'failed';
// v2/PatchChangeKind.ts
export type PatchChangeKind = { type: 'add' } | { type: 'delete' } | { type: 'update'; move_path: string | null };
// v2/FileUpdateChange.ts
export type FileUpdateChange = { path: string; kind: PatchChangeKind; diff: string };
// v2/McpToolCallResult.ts and v2/McpToolCallError.ts
export type McpToolCallResult = { content: unknown[]; structuredContent: unknown };
export type McpToolCallError = { message: string };
// v2/WebSearchAction.ts
export type WebSearchAction =
  | { type: 'search'; query: string | null; queries: string[] | null }
  | { type: 'openPage'; url: string | null }
  | { type: 'findInPage'; url: string | null; pattern: string | null }
  | { type: 'other' };

export type UserMessageItem = { type: 'userMessage'; id: string; clientId: string | null; content: (UserInput | { type: string })[] };
export type AgentMessageItem = { type: 'agentMessage'; id: string; text: string };
export type ReasoningItem = { type: 'reasoning'; id: string; summary: string[]; content: string[] };
export type CommandExecutionItem = {
  type: 'commandExecution'; id: string; command: string; cwd: string; status: CommandExecutionStatus;
  aggregatedOutput: string | null; exitCode: number | null; durationMs: number | null;
};
export type FileChangeItem = { type: 'fileChange'; id: string; changes: FileUpdateChange[]; status: PatchApplyStatus };
export type McpToolCallItem = {
  type: 'mcpToolCall'; id: string; server: string; tool: string; status: McpToolCallStatus; arguments: unknown;
  result: McpToolCallResult | null; error: McpToolCallError | null;
};
// WebSearchItem.ts, flattened into ThreadItem with `type: 'webSearch'`.
export type WebSearchItem = { type: 'webSearch'; id: string; query: string; action: WebSearchAction | null };
/** Every other ThreadItem variant (plan, imageView, contextCompaction, ...): Plait shows none of them. */
export type OtherItem = { type: string; id: string };
export type ThreadItem = UserMessageItem | AgentMessageItem | ReasoningItem | CommandExecutionItem | FileChangeItem | McpToolCallItem | WebSearchItem;

// --- Notifications (ServerNotification.ts) -----------------------------------
// Every one Plait handles carries `threadId`, so one app-server can host several threads.
// The server also adds `emittedAtMs` to each notification frame's top level.

// v2/TurnStartedNotification.ts, v2/TurnCompletedNotification.ts
export type TurnNotification = { threadId: string; turn: Turn };
// v2/ItemStartedNotification.ts, v2/ItemCompletedNotification.ts
export type ItemNotification = { threadId: string; turnId: string; item: ThreadItem | OtherItem };
// v2/AgentMessageDeltaNotification.ts, v2/CommandExecutionOutputDeltaNotification.ts
export type DeltaNotification = { threadId: string; turnId: string; itemId: string; delta: string };
// v2/ReasoningSummaryTextDeltaNotification.ts
export type ReasoningSummaryDeltaNotification = DeltaNotification & { summaryIndex: number };
// v2/ReasoningTextDeltaNotification.ts
export type ReasoningTextDeltaNotification = DeltaNotification & { contentIndex: number };
// v2/FileChangePatchUpdatedNotification.ts
export type FileChangePatchUpdatedNotification = { threadId: string; turnId: string; itemId: string; changes: FileUpdateChange[] };
// v2/TokenUsageBreakdown.ts. `inputTokens` includes cached and cache-write input.
export type TokenUsageBreakdown = {
  totalTokens: number; inputTokens: number; cachedInputTokens: number; cacheWriteInputTokens: number; outputTokens: number; reasoningOutputTokens: number;
};
// v2/ThreadTokenUsage.ts
export type ThreadTokenUsage = { total: TokenUsageBreakdown; last: TokenUsageBreakdown; modelContextWindow: number | null };
// v2/ThreadTokenUsageUpdatedNotification.ts
export type TokenUsageNotification = { threadId: string; turnId: string; tokenUsage: ThreadTokenUsage };
// v2/ErrorNotification.ts
export type ErrorNotification = { error: TurnError; willRetry: boolean; threadId: string; turnId: string };
// v2/ServerRequestResolvedNotification.ts: an approval was settled or withdrawn (for example by an interrupt).
export type ServerRequestResolvedNotification = { threadId: string; requestId: RequestId };

export const NOTIFICATIONS = {
  turnStarted: 'turn/started',
  turnCompleted: 'turn/completed',
  itemStarted: 'item/started',
  itemCompleted: 'item/completed',
  agentMessageDelta: 'item/agentMessage/delta',
  reasoningSummaryDelta: 'item/reasoning/summaryTextDelta',
  reasoningTextDelta: 'item/reasoning/textDelta',
  commandOutputDelta: 'item/commandExecution/outputDelta',
  fileChangePatchUpdated: 'item/fileChange/patchUpdated',
  tokenUsage: 'thread/tokenUsage/updated',
  error: 'error',
  serverRequestResolved: 'serverRequest/resolved',
} as const;

/** The thread a notification or server request belongs to, for routing. */
export function threadOf(params: unknown): string | undefined {
  const id = (params as { threadId?: unknown } | null)?.threadId;
  return typeof id === 'string' ? id : undefined;
}

// --- Server requests (ServerRequest.ts) --------------------------------------

// v2/CommandExecutionApprovalKind.ts: `humanCommand` needs experimentalApi; `writeStdin` is input to a running command.
export type CommandExecutionApprovalKind = 'command' | 'humanCommand' | 'writeStdin';
// v2/CommandExecutionRequestApprovalParams.ts (subset)
export type CommandExecutionRequestApprovalParams = {
  kind: CommandExecutionApprovalKind; threadId: string; turnId: string; itemId: string; approvalId?: string | null;
  reason?: string | null; command?: string | null; cwd?: string | null;
};
// v2/CommandExecutionApprovalDecision.ts (amendment variants not used)
export type CommandExecutionApprovalDecision = 'accept' | 'acceptForSession' | 'decline' | 'cancel';
// v2/CommandExecutionRequestApprovalResponse.ts
export type CommandExecutionRequestApprovalResponse = { decision: CommandExecutionApprovalDecision };
// v2/FileChangeRequestApprovalParams.ts. The changes are on the `fileChange` item with this itemId.
export type FileChangeRequestApprovalParams = { threadId: string; turnId: string; itemId: string; reason?: string | null; grantRoot?: string | null };
// v2/FileChangeApprovalDecision.ts
export type FileChangeApprovalDecision = 'accept' | 'acceptForSession' | 'decline' | 'cancel';
// v2/FileChangeRequestApprovalResponse.ts
export type FileChangeRequestApprovalResponse = { decision: FileChangeApprovalDecision };

// Requests Plait refuses, with the reply each uses to refuse (see approval.ts):
// v2/PermissionsRequestApprovalResponse.ts: grant an empty profile for the turn.
export type PermissionsRequestApprovalResponse = { permissions: Record<string, never>; scope: 'turn' | 'session' };
// v2/ToolRequestUserInputResponse.ts: no answers.
export type ToolRequestUserInputResponse = { answers: Record<string, { answers: string[] }> };
// v2/McpServerElicitationRequestResponse.ts
export type McpServerElicitationRequestResponse = { action: 'accept' | 'decline' | 'cancel'; content: unknown; _meta: unknown };
// v2/DynamicToolCallResponse.ts and v2/DynamicToolCallOutputContentItem.ts (Plait registers no dynamic tools)
export type DynamicToolCallResponse = { contentItems: { type: 'inputText'; text: string }[]; success: boolean };
// ReviewDecision.ts, for the legacy applyPatchApproval and execCommandApproval requests.
export type ReviewDecision = 'approved' | 'approved_for_session' | { denied: { rejection: string } } | 'abort';

export const SERVER_REQUESTS = {
  commandApproval: 'item/commandExecution/requestApproval',
  fileChangeApproval: 'item/fileChange/requestApproval',
  userInput: 'item/tool/requestUserInput',
  elicitation: 'mcpServer/elicitation/request',
  permissions: 'item/permissions/requestApproval',
  dynamicTool: 'item/tool/call',
  tokenRefresh: 'account/chatgptAuthTokens/refresh',
  attestation: 'attestation/generate',
  legacyPatch: 'applyPatchApproval',
  legacyExec: 'execCommandApproval',
} as const;

// --- Models and account ------------------------------------------------------

// v2/ModelListParams.ts
export type ModelListParams = { cursor?: string | null; limit?: number | null; includeHidden?: boolean | null };
// v2/ReasoningEffortOption.ts
export type ReasoningEffortOption = { reasoningEffort: ReasoningEffort; description: string };
// v2/Model.ts (subset)
export type Model = {
  id: string; model: string; displayName: string; description: string; hidden: boolean;
  inputModalities?: string[]; supportedReasoningEfforts: ReasoningEffortOption[]; defaultReasoningEffort: ReasoningEffort; isDefault: boolean;
};
// v2/ModelListResponse.ts
export type ModelListResponse = { data: Model[]; nextCursor: string | null };
// v2/GetAccountParams.ts
export type GetAccountParams = { refreshToken?: boolean };
// v2/Account.ts: Plait reads only the variant tag, never the email or plan.
export type Account = { type: 'apiKey' } | { type: 'chatgpt' } | { type: 'amazonBedrock' } | { type: 'gcp' };
// v2/GetAccountResponse.ts (subset)
export type GetAccountResponse = { account: Account | null; requiresOpenaiAuth: boolean };
