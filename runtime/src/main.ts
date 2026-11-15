import { ModelRuntime } from '@earendil-works/pi-coding-agent';
import { cleanupSessionResources } from '@earendil-works/pi-ai';
import { Runtime } from './runtime.ts';
import { serve } from './stdio.ts';

const models = await ModelRuntime.create({ allowModelNetwork: false });
const runtime = new Runtime(process.argv[2], models, new Set(models.getAvailableSnapshot().map(model => model.provider)));
try { await serve(runtime); }
finally { await runtime.close(); await models.settleCredentialWrites(); cleanupSessionResources(); }
