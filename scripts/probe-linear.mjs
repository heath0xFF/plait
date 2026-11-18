#!/usr/bin/env node
// Opt-in manual probe against Linear's GraphQL API using Plait's LinearClient.
// Usage: LINEAR_API_KEY=lin_api_... node scripts/probe-linear.mjs [TEAM_KEY]
import { validateLinearKey } from '../runtime/src/linear/auth.ts';
import { LinearClient } from '../runtime/src/linear/client.ts';

const rawKey = process.env.LINEAR_API_KEY;
if (!rawKey) {
  console.error('Set LINEAR_API_KEY in the environment to run this probe.');
  process.exit(1);
}

const apiKey = validateLinearKey(rawKey);
const wantedTeam = process.argv[2]?.trim();
const client = new LinearClient();

const teamsResult = await client.listTeams(apiKey);
const team = wantedTeam
  ? teamsResult.data.find(entry => entry.key.toLowerCase() === wantedTeam.toLowerCase() || entry.id === wantedTeam)
  : teamsResult.data[0];

if (!team) {
  console.error(wantedTeam ? `No Linear team matched "${wantedTeam}".` : 'No Linear teams were returned for this key.');
  process.exit(1);
}

const page = await client.listIssues(apiKey, { teamId: team.id, filter: 'active' });
for (const issue of page.data.issues) {
  console.log(`${issue.identifier}\t${issue.title}`);
}
