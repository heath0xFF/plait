// What Plait asks the person when Claude Code's `canUseTool` callback fires,
// and the replies it sends back. Each reply covers one call: Plait never sends
// `updatedPermissions`, so nothing is remembered for the session or saved to
// the person's settings.
import type { CanUseTool, PermissionResult, PermissionUpdate } from '@anthropic-ai/claude-agent-sdk';
import { clipHead } from '../text.ts';

/** `paths` lists an edit's files (absolute, as Claude Code reports them) so the runtime can check them; empty otherwise. */
export type ClaudeApproval = { kind: 'command' | 'edit'; summary: string; details: string; risk: string; paths: string[] };
/** The extra context the SDK passes with a request, as far as Plait shows it. */
export type ApprovalContext = {
  suggestions?: PermissionUpdate[]; title?: string; description?: string; decisionReason?: string;
  blockedPath?: string; mcpServer?: { name: string; source: string };
};

const DETAILS_LIMIT = 8192;
const SUMMARY_LIMIT = 1024;
const FIELD_LIMIT = 4096;
const ONCE = 'Only this request is approved.';
const COMMAND_RISK = `Claude Code runs this command on your machine with your permissions, outside Plait's sandbox. ${ONCE}`;
const EDIT_RISK = `Claude Code writes these changes to the files listed. ${ONCE}`;
const WEB_RISK = `Claude Code reaches the network for this. ${ONCE}`;
const MCP_RISK = `A server from your Claude Code configuration runs this tool, with whatever access that server has. ${ONCE}`;
const PLAN_RISK = 'Claude Code leaves plan mode and starts on the plan under the mode shown in the composer.';
const TOOL_RISK = `Claude Code runs this tool. ${ONCE}`;

/** Tools that ask the person something Plait cannot show yet; they are declined with a note to the model. */
const INTERACTIVE = new Set(['AskUserQuestion']);
const QUESTION_REFUSAL = 'Plait cannot show this question to the person yet. Ask it in your reply instead and continue without it.';
export const DECLINED = 'The person declined this request in Plait.';

// Text from settings and MCP configuration is untrusted: keep printable characters and line breaks only.
const printable = (text: string) => text.replace(/[\u0000-\u0008\u000B-\u001F\u007F-\u009F‪-‮⁦-⁩]/g, '');
function bounded(text: string, limit: number) {
  const clean = printable(text);
  return clean.length > limit ? `${clipHead(clean, limit - 1)}…` : clean;
}
const string = (value: unknown) => typeof value === 'string' ? value : '';
const lines = (...parts: string[]) => parts.filter(Boolean).join('\n');
function json(value: unknown) {
  try { return JSON.stringify(value, null, 2) ?? ''; } catch { return ''; }
}

/** `mcp__server__tool` names the server and tool; server names may themselves hold `__`, tool names usually do not. */
export function mcpName(toolName: string): { server: string; tool: string } | undefined {
  if (!toolName.startsWith('mcp__')) return undefined;
  const rest = toolName.slice(5);
  const split = rest.lastIndexOf('__');
  return split > 0 ? { server: rest.slice(0, split), tool: rest.slice(split + 2) } : { server: rest, tool: rest };
}

function reason(context: ApprovalContext) {
  return lines(
    context.decisionReason ? `Reason: ${bounded(context.decisionReason, FIELD_LIMIT)}` : '',
    context.blockedPath ? `Outside the allowed folders: ${bounded(context.blockedPath, FIELD_LIMIT)}` : '',
  );
}

function command(input: Record<string, unknown>, context: ApprovalContext): ClaudeApproval {
  const text = string(input.command).trim() || 'Run a command';
  const details = lines(
    string(input.description) ? bounded(string(input.description), FIELD_LIMIT) : '',
    input.run_in_background === true ? 'Runs in the background.' : '',
    reason(context),
  );
  return { kind: 'command', summary: bounded(text, SUMMARY_LIMIT), details: bounded(details, DETAILS_LIMIT), risk: COMMAND_RISK, paths: [] };
}

function edit(toolName: string, input: Record<string, unknown>, context: ApprovalContext): ClaudeApproval {
  const path = string(input.file_path) || string(input.notebook_path);
  const name = path || 'a file';
  // Only the head of large content is shown, so it is cut before being split into lines.
  const marked = (sign: string, text: string) => clipHead(text, DETAILS_LIMIT).split('\n').map(line => sign + line).join('\n');
  const minus = (text: string) => marked('-', text);
  const plus = (text: string) => marked('+', text);
  const edits = Array.isArray(input.edits) ? (input.edits as Record<string, unknown>[]).slice(0, 64) : [];
  let summary: string; let body: string;
  switch (toolName) {
    case 'Write':
      summary = `Write ${name}`;
      body = plus(string(input.content));
      break;
    case 'NotebookEdit':
      summary = `${string(input.edit_mode) === 'delete' ? 'Delete a cell in' : 'Edit'} ${name}`;
      body = plus(string(input.new_source));
      break;
    case 'MultiEdit':
      summary = `Edit ${name} in ${edits.length} ${edits.length === 1 ? 'place' : 'places'}`;
      body = edits.map(change => `${minus(string(change?.old_string))}\n${plus(string(change?.new_string))}`).join('\n');
      break;
    default:
      summary = `Edit ${name}${input.replace_all === true ? ' (every match)' : ''}`;
      body = `${minus(string(input.old_string))}\n${plus(string(input.new_string))}`;
  }
  return {
    kind: 'edit', summary: bounded(summary, SUMMARY_LIMIT), details: bounded(lines(reason(context), body), DETAILS_LIMIT),
    risk: EDIT_RISK, paths: path ? [path] : [],
  };
}

function other(toolName: string, input: Record<string, unknown>, context: ApprovalContext): ClaudeApproval {
  const mcp = mcpName(toolName);
  if (toolName === 'WebFetch') {
    return { kind: 'command', summary: bounded(`Fetch ${string(input.url) || 'a page'}`, SUMMARY_LIMIT),
      details: bounded(lines(string(input.prompt), reason(context)), DETAILS_LIMIT), risk: WEB_RISK, paths: [] };
  }
  if (toolName === 'WebSearch') {
    return { kind: 'command', summary: bounded(`Search the web for ${string(input.query) || 'something'}`, SUMMARY_LIMIT),
      details: bounded(reason(context), DETAILS_LIMIT), risk: WEB_RISK, paths: [] };
  }
  if (toolName === 'ExitPlanMode') {
    return { kind: 'command', summary: 'Leave plan mode and start on this plan',
      details: bounded(string(input.plan), DETAILS_LIMIT), risk: PLAN_RISK, paths: [] };
  }
  if (mcp) {
    const server = context.mcpServer?.name || mcp.server;
    return { kind: 'command', summary: bounded(`Use ${mcp.tool} from ${server}`, SUMMARY_LIMIT),
      details: bounded(lines(reason(context), json(input)), DETAILS_LIMIT), risk: MCP_RISK, paths: [] };
  }
  const summary = context.title || (context.description ? `${toolName}: ${context.description}` : `Use ${toolName}`);
  return { kind: 'command', summary: bounded(summary, SUMMARY_LIMIT), details: bounded(lines(reason(context), json(input)), DETAILS_LIMIT), risk: TOOL_RISK, paths: [] };
}

/** Whether a tool asks the person a question rather than asking to run; Plait declines these. */
export function isInteractive(toolName: string): boolean {
  return INTERACTIVE.has(toolName);
}

/**
 * What to ask the person for one `canUseTool` request, or undefined for an
 * interactive request Plait declines with `unsupportedReply`.
 */
export function approvalFor(toolName: string, input: Record<string, unknown>, context: ApprovalContext = {}): ClaudeApproval | undefined {
  if (isInteractive(toolName)) return undefined;
  const args = input && typeof input === 'object' ? input : {};
  switch (toolName) {
    case 'Bash': return command(args, context);
    case 'Edit': case 'MultiEdit': case 'Write': case 'NotebookEdit': return edit(toolName, args, context);
    default: return other(toolName, args, context);
  }
}

/** Allows one call with the input Claude Code asked to run, unchanged. */
export function allowReply(input: Record<string, unknown>): PermissionResult {
  return { behavior: 'allow', updatedInput: input };
}

/** Declines one call; Claude Code tells the model and the turn goes on. */
export function denyReply(message = DECLINED): PermissionResult {
  return { behavior: 'deny', message };
}

/** The reply for a request Plait cannot show. */
export function unsupportedReply(toolName: string): PermissionResult {
  return denyReply(isInteractive(toolName) ? QUESTION_REFUSAL : 'Plait cannot answer this request.');
}

/** Asks the person; resolves true to allow. A rejection or an aborted signal counts as declined. */
export type Ask = (approval: ClaudeApproval, request: { toolUseID: string; toolName: string }, signal: AbortSignal) => Promise<boolean>;

/** The `canUseTool` callback for a session: each request becomes one Plait approval. */
export function canUseToolFrom(ask: Ask): CanUseTool {
  return async (toolName, input, options) => {
    const approval = approvalFor(toolName, input, options);
    if (!approval) return unsupportedReply(toolName);
    const allowed = await ask(approval, { toolUseID: options.toolUseID, toolName }, options.signal).catch(() => false);
    return allowed && !options.signal.aborted ? allowReply(input) : denyReply();
  };
}
