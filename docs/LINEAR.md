# Linear integration options

## Status

Investigation and proposal, not a shipped native integration. Plait has generic
MCP support, but a live Linear connection has not been verified. No Linear
account, issue, credential, or permission was changed for this investigation.

## Start with the existing MCP support

Linear provides an official Streamable HTTP MCP server at
`https://mcp.linear.app/mcp`. Its documentation supports browser OAuth and an
`Authorization: Bearer <token>` header with an API key or OAuth token. A key
with only the Read permission provides a read-only starting point. The `/sse`
endpoint is a deprecated fallback, not the choice for a new connection.

Plait's Pi runtime already accepts configured HTTP headers and makes each tool
call under the thread's approval mode in trusted projects. Sandboxed projects
require explicit server enablement and approval for every call. This makes a
restricted API key a candidate for using Linear without waiting for MCP browser
sign-in. Compatibility still needs a live check against the official server.
Do not mistake a saved Pi sign-in shown in Settings for working token reuse:
Plait's MCP OAuth connection and refresh support remain planned.

Keep credentials in the user's private configuration, never in a repository,
issue, prompt, or example committed to Git. Start read-only, verify the connected
workspace and team, then read one known issue. Add write access only after the
person chooses which operations are needed. Use Ask mode for the first write
checks; changing Linear issues has effects outside the checkout and cannot be
reversed by Plait's Undo.

This first step gives an agent issue context and, when permitted, issue tools.
It does not add a native issue list, a durable thread-to-issue link, automatic
status changes, or an unattended agent.

## Proposed native slice

The smallest useful native integration is **start a thread from an issue**:

1. Associate a Plait project with a chosen Linear workspace, team, and optionally
   a Linear project.
2. Find an issue or paste its URL. Show its identifier, title, state, and source.
3. Let the person review the issue text before using it in a new thread draft.
   Do not start the run automatically.
4. Keep the issue's stable identity and URL with the thread. Show an Open in
   Linear action, not an embedded copy of Linear.
5. Keep posting comments and changing issue state out of the first slice.
   Those can follow with explicit confirmation and clear failure handling.

Issue text is external task data, not permission to run tools or change the
project's policy. Linked files and images need separate bounded reads; do not
send credentials to arbitrary URLs in an issue.

### Ownership

- The host owns credentials, Linear requests, and durable issue links. The
  desktop receives only the data needed to show and select issues.
- Plain models hold issue identity and request state without GPUI. A focused
  integration module handles Linear's API; thread execution does not depend on
  its network availability.
- The desktop owns the picker, draft confirmation, and navigation. It must
  discard stale replies after a project or query changes.
- Use explicit refresh and bounded, paged queries initially. Do not add a
  public webhook listener to the local host or poll each issue separately.

Linear's GraphQL API supports personal API keys and OAuth. For a distributed
native integration, investigate OAuth with PKCE rather than embedding a client
secret. The current docs support PKCE and token refresh. Confirm callback
registration and credential storage before choosing the flow. Handle GraphQL
errors even with HTTP 200, partial results, rate limits, revoked access, and
workspace changes.

This needs agreement on host operations, storage, and authentication before
implementation. No new SDK is selected; if one is used, pin its version and
check its source before using its APIs.

## Decisions before implementation

- Which workspace and team should supply issues? Is an existing Linear project
  used, or should the person create one?
- Is read-only issue selection sufficient for the first native version?
- Should a thread support one source issue or several linked issues? One source
  issue is the smaller first step.
- Which sign-in and credential-storage design should the native integration use?

Using Linear MCP and building native issue navigation are separate steps. The
former can validate the workflow without committing to the latter. Scheduled
runs, issue-triggered execution, and Linear agent delegation remain outside this
proposal; they require the automation permission design in the roadmap.

## Sources

Official documentation consulted for this proposal:

- [Linear MCP](https://linear.app/docs/mcp): endpoint, tools, header
  authentication, read-only keys, and separate workspace authentication.
- [GraphQL API](https://linear.app/developers/graphql): queries, issue IDs,
  authentication, partial errors, and update guidance. Note that a personal API
  key uses `Authorization: <key>` for GraphQL, unlike the MCP Bearer header.
- [OAuth authentication](https://linear.app/developers/oauth-2-0-authentication):
  PKCE, scopes, refresh, and revocation.

These sources establish supported interfaces, not end-to-end Plait verification.
