import { fail } from '../protocol.ts';
import { prepareCommand } from './macos.ts';

export function commandPlatform(platform: NodeJS.Platform = process.platform) {
  switch (platform) {
    case 'darwin':
      return { scratchPrefix: '/private/tmp/plait-command-', prepare: prepareCommand };
    default:
      // Missing containment is a denial, never permission to use an ordinary shell.
      fail('permission_denied', 'Commands require the tested macOS filesystem sandbox. Linux command containment is not enabled.');
  }
}
