import { InMemoryCredentialStore, envApiKeyAuth } from '@earendil-works/pi-ai';
import type { Api, AuthInteraction, Model, OAuthCredential, Provider } from '@earendil-works/pi-ai';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { failure } from '../../src/protocol.ts';
import { Providers } from '../../src/providers.ts';
import type { ProviderSource, Registry } from '../../src/providers.ts';
import type { SignInLimits } from '../../src/sign-in.ts';

// Scripted stand-ins for Pi's provider flows. Nothing here contacts a provider; the
// addresses use the reserved .invalid domain and are never opened.
export const SECRETS = {
  access: 'ACCESS_TOKEN_DO_NOT_LEAK', refresh: 'REFRESH_TOKEN_DO_NOT_LEAK',
  body: 'PROVIDER_ERROR_BODY_DO_NOT_LEAK', code: 'good-code-DO_NOT_ECHO',
};
export const LOOPBACK_URL = 'https://example.invalid/authorize?redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fcallback&state=one-time';
export const COPY_URL = 'https://example.invalid/authorize?redirect_uri=https%3A%2F%2Fexample.invalid%2Fcode';
export const DEVICE_URL = 'https://example.invalid/device';

const never = () => { throw new Error('Scripted provider has no model transport'); };
const template = fauxProvider().models[0];
function provider(id: string, name: string, auth: Provider['auth'], models = [`${id}-model`]): Provider {
  const listed = models.map(model => ({ ...template, id: model, name: model, provider: id }) as Model<Api>);
  return { id, name, auth, getModels: () => listed, stream: never, streamSimple: never };
}

export type FlowLog = {
  started: number; aborted: number;
  /** Set by a waiting device flow; calling it stands in for the user approving on the provider's page. */
  approve?: () => void;
  /** Runs before the browser address is announced, where Pi starts its loopback callback listener. */
  listen?: (interaction: AuthInteraction) => Promise<void>;
};

/**
 * Follows the shape of Pi 1.5.2's subscription flows: a method choice, then either a browser
 * address with an optional pasted code, a copied code, or a device code.
 */
function scriptedLogin(log: FlowLog) {
  return async (interaction: AuthInteraction): Promise<OAuthCredential> => {
    log.started++;
    interaction.signal?.addEventListener('abort', () => { log.aborted++; }, { once: true });
    const credential = (): OAuthCredential => ({ type: 'oauth', access: SECRETS.access, refresh: SECRETS.refresh, expires: Date.now() + 3600_000 });
    const method = await interaction.prompt({
      type: 'select', message: 'Select login method:',
      options: [{ id: 'browser', label: 'Browser login (default)' }, { id: 'copy_code', label: 'Copy code login (headless)' }, { id: 'device_code', label: 'Device code login (headless)' }, { id: 'broken', label: 'Scripted failure' }],
    });
    if (method === 'broken') throw new Error(`token exchange failed: body={"refresh_token":"${SECRETS.body}"}`);
    if (method === 'device_code') {
      interaction.notify({ type: 'device_code', userCode: 'ABCD-1234', verificationUri: DEVICE_URL, expiresInSeconds: 900 });
      // The scripted device flow waits until the test approves it or the sign-in is stopped.
      await new Promise<void>((resolve, reject) => {
        interaction.signal?.addEventListener('abort', () => reject(new Error('Login cancelled')), { once: true });
        log.approve = () => resolve();
      });
      interaction.notify({ type: 'progress', message: 'Enabling models...' });
      return credential();
    }
    interaction.notify({ type: 'progress', message: 'Listening for the callback' });
    await log.listen?.(interaction);
    interaction.notify({ type: 'auth_url', url: method === 'browser' ? LOOPBACK_URL : COPY_URL, instructions: 'Complete login in your browser.' });
    const code = await interaction.prompt({ type: 'manual_code', message: 'Paste the authorization code:', placeholder: 'code#state' });
    if (code !== SECRETS.code) throw new Error(`Token exchange request failed. body={"error":"${SECRETS.body}"} code=${code}`);
    interaction.notify({ type: 'progress', message: 'Exchanging authorization code for tokens...' });
    return credential();
  };
}

export function scriptedSource(extra: () => Provider[] = () => []) {
  const credentials = new InMemoryCredentialStore();
  const flows: Record<string, FlowLog> = { subscription: { started: 0, aborted: 0 }, both: { started: 0, aborted: 0 }, second: { started: 0, aborted: 0 }, third: { started: 0, aborted: 0 }, fourth: { started: 0, aborted: 0 }, fifth: { started: 0, aborted: 0 } };
  const oauth = (id: string, name: string) => ({
    name, isSubscription: true, login: scriptedLogin(flows[id]),
    refresh: async (credential: OAuthCredential) => credential, toAuth: async (credential: OAuthCredential) => ({ apiKey: credential.access }),
  });
  const state = { loads: 0, fail: false, delay: 0 };
  const source: ProviderSource = {
    builtin: new Set(['subscription', 'both', 'keyed', 'ambient', 'second', 'third', 'fourth', 'fifth', 'faux']),
    async load(): Promise<Registry> {
      const load = ++state.loads;
      // Read at the start, so a slow load holds what an overtaken reload would have seen.
      const added = extra();
      if (state.delay) await new Promise(resolve => setTimeout(resolve, state.delay));
      if (state.fail) throw new Error(`load failed near ${SECRETS.body}`);
      const models = createModels({ credentials });
      models.setProvider(provider('subscription', 'Scripted Subscription', { oauth: oauth('subscription', 'Scripted (subscription)') }));
      models.setProvider(provider('both', 'Scripted Both', { oauth: oauth('both', 'Scripted Both (subscription)'), apiKey: envApiKeyAuth('Scripted Both API key', []) }));
      models.setProvider(provider('keyed', 'Scripted Keyed', { apiKey: envApiKeyAuth('Scripted API key', []) }, ['keyed-a', 'keyed-b']));
      // Authenticated without a stored credential, as an environment variable would make it.
      models.setProvider(provider('ambient', 'Scripted Ambient', { apiKey: { name: 'Ambient', resolve: async () => ({ auth: {} }) } }));
      for (const id of ['second', 'third', 'fourth', 'fifth']) models.setProvider(provider(id, id, { oauth: oauth(id, id) }));
      for (const item of added) models.setProvider(item);
      return {
        models, authenticated: new Set((await models.getAvailable()).map(model => model.provider)),
        stored: async () => new Map((await credentials.list()).map(entry => [entry.providerId, entry.type])),
        settle: async () => { void load; },
      };
    },
  };
  return { source, credentials, flows, state };
}

/** A `Providers` over scripted providers, with replies wrapped as the host wraps them. */
export async function scripted(limits: Partial<SignInLimits> = {}, extra?: () => Provider[]) {
  const fake = scriptedSource(extra);
  const registry = await fake.source.load();
  const holder = { models: registry.models, authenticated: registry.authenticated };
  const providers = new Providers(holder, fake.source, registry, limits);
  // Every reply is kept so a test can prove that no secret appeared in any of them.
  const transcript: string[] = [];
  const call = async <T = Record<string, any>>(input: unknown): Promise<{ ok: boolean; result: T; error?: { code: string; message: string } }> => {
    let reply;
    try { reply = { ok: true, result: await providers.handle(input) }; } catch (error) { reply = { ok: false, error: failure(error) }; }
    transcript.push(JSON.stringify(reply));
    return reply as { ok: boolean; result: T; error?: { code: string; message: string } };
  };
  return { ...fake, providers, holder, call, transcript };
}

export function scriptedProvider(id: string, models: string[]): Provider {
  return provider(id, id, { apiKey: { name: 'Scripted', resolve: async () => ({ auth: {} }) } }, models);
}
