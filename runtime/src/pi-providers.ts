import { join } from 'node:path';
import { ModelRuntime, SettingsManager, getAgentDir } from '@earendil-works/pi-coding-agent';
import { builtinProviders } from '@earendil-works/pi-ai/providers/all';
import type { ProviderSource, Registry } from './providers.ts';

/**
 * Pi's own files: auth.json for credentials and models.json for custom endpoints, both in
 * Pi's agent directory. Each load reads them again and never refreshes catalogs over the network.
 */
export function piProviders(agentDir = getAgentDir()): ProviderSource {
  const authPath = join(agentDir, 'auth.json');
  const modelsPath = join(agentDir, 'models.json');
  return {
    builtin: new Set(builtinProviders().map(provider => provider.id)),
    authPath, modelsPath,
    // Pi's ChatGPT sign-in sends the installation's device ID, which Pi keeps in its settings.
    loginOptions: { getDeviceId: () => SettingsManager.create(process.cwd(), agentDir, { projectTrusted: false }).getOrCreateDeviceId() },
    async load(signal?: AbortSignal): Promise<Registry> {
      const models = await ModelRuntime.create({ authPath, modelsPath, allowModelNetwork: false, signal });
      // An interrupted load has an incomplete authenticated set; keep the previous registry instead.
      if (signal?.aborted) throw new Error('Provider load timed out');
      return {
        models,
        authenticated: new Set(models.getAvailableSnapshot().map(model => model.provider)),
        stored: async () => new Map((await models.listCredentials()).map(entry => [entry.providerId, entry.type])),
        settle: () => models.settleCredentialWrites(),
      };
    },
  };
}
