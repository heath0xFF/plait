import { SERVER_REQUESTS } from './protocol.ts';
import type {
  CommandExecutionRequestApprovalParams, CommandExecutionRequestApprovalResponse, DynamicToolCallResponse, FileChangeItem,
  FileChangeRequestApprovalParams, FileChangeRequestApprovalResponse, McpServerElicitationRequestResponse,
  PermissionsRequestApprovalResponse, ReviewDecision, ToolRequestUserInputResponse,
} from './protocol.ts';
import { clipHead } from '../text.ts';

/** `paths` lists an edit's files (absolute, as Codex reports them) so the runtime can check them; empty for commands. */
export type CodexApproval = { kind: 'command' | 'edit'; summary: string; details: string; risk: string; paths: string[] };

const DETAILS_LIMIT = 8192;
const SUMMARY_LIMIT = 1024;
const COMMAND_RISK = 'Codex runs this command with the sandbox and network access it was started with. Only this request is approved.';
const STDIN_RISK = 'Codex sends this input to a command that is already running. Only this request is approved.';
const EDIT_RISK = 'Codex writes these changes to the files listed. Only this request is approved.';

function bounded(text: string, limit: number) {
  return text.length > limit ? `${clipHead(text, limit - 1)}…` : text;
}

function commandApproval(params: CommandExecutionRequestApprovalParams): CodexApproval {
  const stdin = params.kind === 'writeStdin';
  const command = params.command?.trim() || (stdin ? 'Send input to a running command' : 'Run a command');
  const details = [params.cwd ? `In ${params.cwd}` : '', params.reason ? `Reason: ${params.reason}` : ''].filter(Boolean).join('\n');
  return { kind: 'command', summary: bounded(command, SUMMARY_LIMIT), details: bounded(details, DETAILS_LIMIT), risk: stdin ? STDIN_RISK : COMMAND_RISK, paths: [] };
}

/** The request carries no changes; they are on the `fileChange` item it names. */
function editApproval(params: FileChangeRequestApprovalParams, item: FileChangeItem | undefined): CodexApproval {
  const changes = item?.changes ?? [];
  const count = changes.length;
  const summary = count ? `Edit ${count} ${count === 1 ? 'file' : 'files'}` : 'Edit files';
  const lines = [
    ...(params.reason ? [`Reason: ${params.reason}`] : []),
    ...(params.grantRoot ? [`Asks to write anywhere under ${params.grantRoot} for this session.`] : []),
    ...changes.map(change => `${change.kind.type === 'add' ? 'Add' : change.kind.type === 'delete' ? 'Delete' : 'Update'} ${change.path}`),
    ...changes.map(change => change.diff).filter(Boolean),
  ];
  return { kind: 'edit', summary, details: bounded(lines.join('\n'), DETAILS_LIMIT), risk: EDIT_RISK, paths: changes.map(change => change.path) };
}

/**
 * What to ask the person for one Codex approval request, or undefined when the
 * method is not an approval Plait presents. Pass the started `fileChange` item
 * for an edit request so its paths and diff can be shown.
 */
export function approvalFor(method: string, params: any, item?: FileChangeItem): CodexApproval | undefined {
  if (method === SERVER_REQUESTS.commandApproval) return commandApproval(params as CommandExecutionRequestApprovalParams);
  if (method === SERVER_REQUESTS.fileChangeApproval) return editApproval(params as FileChangeRequestApprovalParams, item);
  return undefined;
}

/** The reply for one decision. Each approves this request only, never the session. */
export function approvalReply(method: string, allow: boolean): CommandExecutionRequestApprovalResponse | FileChangeRequestApprovalResponse {
  if (method !== SERVER_REQUESTS.commandApproval && method !== SERVER_REQUESTS.fileChangeApproval) {
    throw new Error(`${method} is not an approval Plait answers.`);
  }
  return { decision: allow ? 'accept' : 'decline' };
}

const REFUSAL = 'Plait does not support this request.';

/**
 * A reply that safely refuses a server request Plait does not support. Undefined
 * means there is no refusing result; answer with a JSON-RPC error instead.
 */
export function declineUnsupported(method: string): unknown | undefined {
  switch (method) {
    // Grant nothing extra; the turn continues with its existing permissions.
    case SERVER_REQUESTS.permissions: return { permissions: {}, scope: 'turn' } satisfies PermissionsRequestApprovalResponse;
    // No answers for any question.
    case SERVER_REQUESTS.userInput: return { answers: {} } satisfies ToolRequestUserInputResponse;
    case SERVER_REQUESTS.elicitation: return { action: 'decline', content: null, _meta: null } satisfies McpServerElicitationRequestResponse;
    // Plait registers no dynamic tools, so a call is answered as failed.
    case SERVER_REQUESTS.dynamicTool: return { contentItems: [{ type: 'inputText', text: REFUSAL }], success: false } satisfies DynamicToolCallResponse;
    // Legacy approvals from the v1 API, which Plait does not use.
    case SERVER_REQUESTS.legacyExec:
    case SERVER_REQUESTS.legacyPatch: return { decision: { denied: { rejection: REFUSAL } } satisfies ReviewDecision };
    // Token refresh and attestation have no refusing result; Plait opts out of both.
    default: return undefined;
  }
}
